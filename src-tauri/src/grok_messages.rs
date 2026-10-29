//! Watches Grok Bot's local reply cache without modifying it or saving messages.
//! The cache is an undocumented format, so malformed or missing files are ignored.
use serde_json::Value;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const ACCOUNT_PREFIX: &str = "sand.client.slice.account.grok%7C";
const ROSTER_SUFFIX: &str = ".roster.last-roster";
const MAX_ROSTER_BYTES: u64 = 4 * 1024 * 1024;
const MAX_TRANSCRIPT_BYTES: u64 = 16 * 1024 * 1024;
const STABLE_MS: u64 = 1_500;

#[derive(Default)]
struct Account {
    seen: HashMap<String, String>,
    pending: HashMap<String, Pending>,
}
struct Pending {
    id: String,
    preview: String,
    content_fingerprint: u64,
    since_ms: u64,
}
struct Watcher {
    started_ms: u64,
    accounts: HashMap<String, Account>,
}

impl Watcher {
    fn new(started_ms: u64) -> Self {
        Self {
            started_ms,
            accounts: HashMap::new(),
        }
    }

    // A new account starts with a baseline. Only a later, confirmed assistant
    // message is emitted; contents live in memory for the display interval only.
    fn observe<F>(
        &mut self,
        account_key: &str,
        roster: &Value,
        now_ms: u64,
        mut transcript: F,
    ) -> Vec<String>
    where
        F: FnMut(&str) -> Option<Value>,
    {
        let Some(rows) = roster.pointer("/value/rows").and_then(Value::as_array) else {
            return Vec::new();
        };
        if !self.accounts.contains_key(account_key) {
            let mut account = Account::default();
            for row in rows {
                if let Some((agent, id)) = row_identity(row) {
                    account.seen.insert(agent.to_owned(), id.to_owned());
                }
            }
            self.accounts.insert(account_key.to_owned(), account);
            return Vec::new();
        }
        let account = self
            .accounts
            .get_mut(account_key)
            .expect("account inserted");
        let mut replies = Vec::new();
        for row in rows {
            let Some((agent, id)) = row_identity(row) else {
                continue;
            };
            let previous = account.seen.get(agent);
            if previous.is_some_and(|value| value == id) {
                account.pending.remove(agent);
                continue;
            }
            // A newly discovered old conversation must not replay its history.
            if previous.is_none()
                && row
                    .get("lastActivityAt")
                    .and_then(Value::as_u64)
                    .unwrap_or(0)
                    < self.started_ms
            {
                account.seen.insert(agent.to_owned(), id.to_owned());
                continue;
            }
            if !assistant_message_id(id) {
                account.seen.insert(agent.to_owned(), id.to_owned());
                account.pending.remove(agent);
                continue;
            }
            let Some(data) = transcript(agent) else {
                continue;
            };
            let Some(entries) = data.pointer("/value/entries").and_then(Value::as_array) else {
                continue;
            };
            let Some(entry) = entries
                .iter()
                .rev()
                .find(|entry| entry.get("id").and_then(Value::as_str) == Some(id))
            else {
                continue;
            };
            if entry.get("kind").and_then(Value::as_str) != Some("send-message")
                || entry.pointer("/message/type").and_then(Value::as_str) != Some("text")
            {
                account.seen.insert(agent.to_owned(), id.to_owned());
                account.pending.remove(agent);
                continue;
            }
            if entry
                .get("timestampMs")
                .and_then(Value::as_u64)
                .is_some_and(|time| time < self.started_ms)
            {
                account.seen.insert(agent.to_owned(), id.to_owned());
                account.pending.remove(agent);
                continue;
            }
            let Some(content) = entry
                .pointer("/message/content")
                .and_then(Value::as_str)
                .filter(|text| !text.trim().is_empty())
            else {
                continue;
            };
            let name = row.get("name").and_then(Value::as_str).unwrap_or("");
            let preview = crate::island_notifications::preview(name, content);
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            content.hash(&mut hasher);
            let content_fingerprint = hasher.finish();
            match account.pending.get(agent) {
                Some(pending)
                    if pending.id == id
                        && pending.preview == preview
                        && pending.content_fingerprint == content_fingerprint
                        && now_ms.saturating_sub(pending.since_ms) >= STABLE_MS =>
                {
                    replies.push(preview);
                    account.seen.insert(agent.to_owned(), id.to_owned());
                    account.pending.remove(agent);
                }
                Some(pending)
                    if pending.id == id
                        && pending.preview == preview
                        && pending.content_fingerprint == content_fingerprint => {}
                _ => {
                    account.pending.insert(
                        agent.to_owned(),
                        Pending {
                            id: id.to_owned(),
                            preview,
                            content_fingerprint,
                            since_ms: now_ms,
                        },
                    );
                }
            }
        }
        replies
    }
}

fn row_identity(row: &Value) -> Option<(&str, &str)> {
    let agent = row.get("id")?.as_str()?;
    if agent.is_empty()
        || agent.len() > 100
        || !agent
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return None;
    }
    let id = row.get("newestEntryId")?.as_str()?;
    (id.len() <= 100).then_some((agent, id))
}

fn assistant_message_id(id: &str) -> bool {
    let Some((turn, sequence)) = id.strip_prefix('t').and_then(|rest| rest.split_once('s')) else {
        return false;
    };
    !turn.is_empty()
        && !sequence.is_empty()
        && turn.bytes().all(|b| b.is_ascii_digit())
        && sequence.bytes().all(|b| b.is_ascii_digit())
}

fn decode_base32(value: &str) -> Option<String> {
    let mut data = Vec::with_capacity(value.len() * 5 / 8);
    let (mut accumulator, mut bits) = (0u32, 0u32);
    for byte in value.bytes() {
        let n = match byte {
            b'a'..=b'z' => byte - b'a',
            b'A'..=b'Z' => byte - b'A',
            b'2'..=b'7' => byte - b'2' + 26,
            _ => return None,
        };
        accumulator = (accumulator << 5) | u32::from(n);
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            data.push((accumulator >> bits) as u8);
            accumulator &= (1 << bits) - 1;
        }
    }
    if accumulator != 0 {
        return None;
    }
    String::from_utf8(data).ok()
}

fn encode_base32(value: &str) -> String {
    const ALPHABET: &[u8; 32] = b"abcdefghijklmnopqrstuvwxyz234567";
    let mut result = String::with_capacity((value.len() * 8 + 4) / 5);
    let (mut accumulator, mut bits) = (0u32, 0u32);
    for byte in value.bytes() {
        accumulator = (accumulator << 8) | u32::from(byte);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            result.push(ALPHABET[((accumulator >> bits) & 31) as usize] as char);
            accumulator &= (1 << bits) - 1;
        }
    }
    if bits > 0 {
        result.push(ALPHABET[(accumulator << (5 - bits)) as usize] as char);
    }
    result
}

#[cfg(windows)]
fn source_path(path: &Path) -> PathBuf {
    use std::ffi::OsString;
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    let wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    if !path.is_absolute() || wide.starts_with(&[92, 92, 63, 92]) {
        return path.to_owned();
    }
    let mut prefix: Vec<u16> = if wide.starts_with(&[92, 92]) {
        r"\\?\UNC\".encode_utf16().collect()
    } else {
        r"\\?\".encode_utf16().collect()
    };
    prefix.extend_from_slice(if wide.starts_with(&[92, 92]) {
        &wide[2..]
    } else {
        &wide
    });
    PathBuf::from(OsString::from_wide(&prefix))
}
#[cfg(not(windows))]
fn source_path(path: &Path) -> PathBuf {
    path.to_owned()
}

fn read_json(path: &Path, limit: u64) -> Option<Value> {
    let path = source_path(path);
    let file_type = std::fs::symlink_metadata(&path).ok()?.file_type();
    if file_type.is_symlink() || !file_type.is_file() {
        return None;
    }
    let file = std::fs::File::open(path).ok()?;
    let mut bytes = Vec::new();
    file.take(limit + 1).read_to_end(&mut bytes).ok()?;
    if bytes.len() as u64 > limit {
        return None;
    }
    serde_json::from_slice(&bytes).ok()
}

#[cfg(windows)]
fn scan(root: &Path, watcher: &mut Watcher, elapsed_ms: u64) -> (bool, Vec<String>) {
    let Ok(entries) = std::fs::read_dir(source_path(root)) else {
        return (false, Vec::new());
    };
    let mut ready = false;
    let mut messages = Vec::new();
    for entry in entries.take(4096).flatten() {
        let name = entry.file_name();
        let Some(encoded) = name.to_str().and_then(|name| name.strip_suffix(".blob")) else {
            continue;
        };
        let Some(key) = decode_base32(encoded) else {
            continue;
        };
        let Some(account_key) = key
            .strip_suffix(ROSTER_SUFFIX)
            .filter(|key| key.starts_with(ACCOUNT_PREFIX))
        else {
            continue;
        };
        let Some(roster) = read_json(&entry.path(), MAX_ROSTER_BYTES) else {
            continue;
        };
        if !roster.pointer("/value/rows").is_some_and(Value::is_array) {
            continue;
        }
        ready = true;
        messages.extend(watcher.observe(account_key, &roster, elapsed_ms, |agent| {
            let key = format!("{account_key}.transcript.replicas.{agent}");
            let path = root.join(format!("{}.blob", encode_base32(&key)));
            read_json(&path, MAX_TRANSCRIPT_BYTES)
        }));
    }
    (ready, messages)
}

#[cfg(windows)]
pub(crate) fn start_listener(app: tauri::AppHandle) {
    use std::time::Instant;
    use tauri::Manager;
    let Some(roaming) = std::env::var_os("APPDATA") else {
        return;
    };
    let root = PathBuf::from(roaming)
        .join("Grok Bot")
        .join("sand-client-persistence");
    let _ = std::thread::Builder::new()
        .name("grok-messages".into())
        .spawn(move || {
            let mut watcher: Option<Watcher> = None;
            let mut started = Instant::now();
            loop {
                if app.state::<crate::desktop::DesktopState>().is_quitting() {
                    return;
                }
                if crate::island_notifications::messages_enabled(&app) {
                    let watcher = watcher.get_or_insert_with(|| {
                        started = Instant::now();
                        Watcher::new(
                            SystemTime::now()
                                .duration_since(UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_millis() as u64,
                        )
                    });
                    let (_, replies) =
                        scan(&root, watcher, started.elapsed().as_millis() as u64);
                    for reply in replies {
                        crate::island_notifications::emit_grok_reply(&app, reply);
                    }
                } else {
                    watcher = None;
                }
                std::thread::sleep(Duration::from_millis(750));
            }
        });
}
#[cfg(not(windows))]
pub(crate) fn start_listener(_app: tauri::AppHandle) {}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn roster(id: &str) -> Value {
        json!({"value": {"rows": [{"id": "agent-1", "name": "小助手", "newestEntryId": id, "lastActivityAt": 1_100}]}})
    }
    fn transcript(id: &str, kind: &str, content: &str) -> Value {
        json!({"value": {"entries": [{"id": id, "kind": kind, "timestampMs": 1_100, "message": {"type": "text", "content": content}}]}})
    }
    #[test]
    fn rfc4648_names_and_long_keys_round_trip() {
        for (raw, encoded) in [("foo", "mzxw6"), ("foobar", "mzxw6ytboi")] {
            assert_eq!(encode_base32(raw), encoded);
            assert_eq!(decode_base32(encoded).as_deref(), Some(raw));
        }
        let key = format!("{ACCOUNT_PREFIX}user_123.transcript.replicas.agent-1");
        assert_eq!(
            decode_base32(&encode_base32(&key)).as_deref(),
            Some(key.as_str())
        );
        assert!(decode_base32("bad!").is_none());
    }
    #[test]
    fn only_new_confirmed_stable_assistant_text_is_emitted_once() {
        let mut watcher = Watcher::new(1_000);
        let mut reads = 0;
        let initial = watcher.observe("a", &roster("t1s1"), 0, |_| {
            reads += 1;
            None
        });
        assert!(initial.is_empty());
        assert_eq!(reads, 0);
        let reply = transcript("t2s2", "send-message", "新回复");
        assert!(watcher
            .observe("a", &roster("t2s2"), 100, |_| Some(reply.clone()))
            .is_empty());
        assert!(watcher
            .observe("a", &roster("t2s2"), 1_000, |_| Some(reply.clone()))
            .is_empty());
        assert_eq!(
            watcher.observe("a", &roster("t2s2"), 1_700, |_| Some(reply.clone())),
            vec!["小助手：新回复"]
        );
        assert!(watcher
            .observe("a", &roster("t2s2"), 4_000, |_| panic!("already seen"))
            .is_empty());
        assert!(watcher
            .observe("a", &roster("t3u"), 5_000, |_| panic!("user message"))
            .is_empty());
        assert!(watcher
            .observe("a", &roster("t4s1"), 6_000, |_| Some(transcript(
                "t4s1",
                "message",
                "用户提问"
            )))
            .is_empty());
    }
    #[test]
    fn changed_content_restarts_stability_timer_and_old_rows_are_not_replayed() {
        let mut watcher = Watcher::new(1_000);
        watcher.observe("a", &roster("t1s1"), 0, |_| None);
        watcher.observe("a", &roster("t2s1"), 100, |_| {
            Some(transcript("t2s1", "send-message", "开始"))
        });
        watcher.observe("a", &roster("t2s1"), 1_700, |_| {
            Some(transcript("t2s1", "send-message", "完整回复"))
        });
        assert!(watcher
            .observe("a", &roster("t2s1"), 2_000, |_| Some(transcript(
                "t2s1",
                "send-message",
                "完整回复"
            )))
            .is_empty());
        assert_eq!(
            watcher.observe("a", &roster("t2s1"), 3_200, |_| Some(transcript(
                "t2s1",
                "send-message",
                "完整回复"
            ))),
            vec!["小助手：完整回复"]
        );
        let old = json!({"value": {"rows": [{"id": "old-agent", "newestEntryId": "t1s1", "lastActivityAt": 999}]}});
        assert!(watcher
            .observe("a", &old, 4_000, |_| panic!("old conversation"))
            .is_empty());
    }

    #[test]
    fn waits_for_matching_transcript_and_rejects_unrelated_entries() {
        let mut watcher = Watcher::new(1_000);
        watcher.observe("a", &roster("t1s1"), 0, |_| None);
        assert!(watcher
            .observe("a", &roster("t2s2"), 100, |_| None)
            .is_empty());
        assert!(watcher
            .observe("a", &roster("t2s2"), 2_100, |_| Some(transcript(
                "t9s9",
                "send-message",
                "旧内容"
            )))
            .is_empty());
        let reply = transcript("t2s2", "send-message", "新内容");
        assert!(watcher
            .observe("a", &roster("t2s2"), 4_100, |_| Some(reply.clone()))
            .is_empty());
        assert_eq!(
            watcher.observe("a", &roster("t2s2"), 6_100, |_| Some(reply.clone())),
            vec!["小助手：新内容"]
        );
    }

    #[test]
    fn content_changes_beyond_visible_preview_restart_the_timer() {
        let mut watcher = Watcher::new(1_000);
        watcher.observe("a", &roster("t1s1"), 0, |_| None);
        let first = format!("{}甲", "长".repeat(100));
        let second = format!("{}乙", "长".repeat(100));
        watcher.observe("a", &roster("t2s1"), 100, |_| {
            Some(transcript("t2s1", "send-message", &first))
        });
        assert!(watcher
            .observe("a", &roster("t2s1"), 1_700, |_| Some(transcript(
                "t2s1",
                "send-message",
                &second
            )))
            .is_empty());
        assert!(watcher
            .observe("a", &roster("t2s1"), 2_000, |_| Some(transcript(
                "t2s1",
                "send-message",
                &second
            )))
            .is_empty());
        assert_eq!(
            watcher
                .observe("a", &roster("t2s1"), 3_200, |_| Some(transcript(
                    "t2s1",
                    "send-message",
                    &second
                )))
                .len(),
            1
        );
    }

    #[cfg(windows)]
    #[test]
    fn discovers_and_reads_a_long_path_using_local_fixture_files() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let parent =
            std::env::temp_dir().join(format!("codex-flow-grok-{}-{nonce}", std::process::id()));
        let root = parent.join("x".repeat(140));
        std::fs::create_dir_all(source_path(&root)).unwrap();
        let key = format!("{ACCOUNT_PREFIX}user_test");
        let roster_file = root.join(format!(
            "{}.blob",
            encode_base32(&format!("{key}{ROSTER_SUFFIX}"))
        ));
        let transcript_file = root.join(format!(
            "{}.blob",
            encode_base32(&format!("{key}.transcript.replicas.agent-1"))
        ));
        assert!(transcript_file.to_string_lossy().len() > 260);
        std::fs::write(source_path(&roster_file), roster("t1s1").to_string()).unwrap();
        let mut watcher = Watcher::new(1_000);
        assert!(scan(&root, &mut watcher, 0).1.is_empty());
        std::fs::write(source_path(&roster_file), roster("t2s2").to_string()).unwrap();
        std::fs::write(
            source_path(&transcript_file),
            transcript("t2s2", "send-message", "测试回复").to_string(),
        )
        .unwrap();
        assert!(scan(&root, &mut watcher, 100).1.is_empty());
        assert_eq!(scan(&root, &mut watcher, 1_700).1, vec!["小助手：测试回复"]);
        std::fs::remove_file(source_path(&transcript_file)).unwrap();
        std::fs::remove_file(source_path(&roster_file)).unwrap();
        std::fs::remove_dir(source_path(&root)).unwrap();
        std::fs::remove_dir(source_path(&parent)).unwrap();
    }
}
