//! Installs an opt-in, per-user Codex Stop hook without replacing other hooks.
use serde_json::{json, Value};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::WebviewWindow;

const MAX_CONFIG_BYTES: u64 = 1024 * 1024;
const HOOK_MARKER: &str = "--codex-flow-stop-hook";

fn hook_document(raw: Option<&[u8]>, exe: &Path) -> Result<Option<Vec<u8>>, &'static str> {
    let name = exe.to_str().ok_or("CODEX_PATH_INVALID")?;
    if !exe.is_absolute() || name.chars().any(char::is_control) {
        return Err("CODEX_PATH_INVALID");
    }
    // Codex uses PowerShell for its Windows hooks. A literal single-quoted path
    // prevents expansion of spaces, quotes, dollar signs and shell metacharacters.
    let command = format!("& '{}' {HOOK_MARKER}", name.replace('\'', "''"));
    let mut doc: Value = match raw {
        Some(bytes) => serde_json::from_slice(bytes).map_err(|_| "CODEX_CONFIG_INVALID")?,
        None => json!({}),
    };
    let root = doc.as_object_mut().ok_or("CODEX_CONFIG_INVALID")?;
    let hooks = root
        .entry("hooks")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or("CODEX_CONFIG_INVALID")?;
    let stop = hooks
        .entry("Stop")
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .ok_or("CODEX_CONFIG_INVALID")?;
    if stop
        .iter()
        .filter_map(|group| group.get("hooks").and_then(Value::as_array))
        .flatten()
        .any(|handler| {
            ["command", "commandWindows", "command_windows"]
                .iter()
                .any(|key| {
                    handler
                        .get(key)
                        .and_then(Value::as_str)
                        .is_some_and(|s| s.contains(HOOK_MARKER))
                })
        })
    {
        return Ok(None);
    }
    stop.push(json!({"hooks": [{"type": "command", "command": command, "commandWindows": command, "timeout": 3}]}));
    let mut contents = serde_json::to_vec_pretty(&doc).map_err(|_| "CODEX_CONFIG_INVALID")?;
    contents.push(b'\n');
    Ok(Some(contents))
}

fn read_existing(path: &Path) -> Result<Option<Vec<u8>>, &'static str> {
    match fs::symlink_metadata(path) {
        Ok(meta) => {
            if meta.file_type().is_symlink() || !meta.is_file() || meta.len() > MAX_CONFIG_BYTES {
                return Err("CODEX_CONFIG_INVALID");
            }
            let file = fs::File::open(path).map_err(|_| "CODEX_CONFIG_READ_FAILED")?;
            let mut bytes = Vec::new();
            file.take(MAX_CONFIG_BYTES + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| "CODEX_CONFIG_READ_FAILED")?;
            if bytes.len() as u64 > MAX_CONFIG_BYTES {
                return Err("CODEX_CONFIG_INVALID");
            }
            Ok(Some(bytes))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err("CODEX_CONFIG_READ_FAILED"),
    }
}

fn install_hook(path: &Path, exe: &Path) -> Result<&'static str, &'static str> {
    let original = read_existing(path)?;
    let Some(updated) = hook_document(original.as_deref(), exe)? else {
        return Ok("existing");
    };
    let parent = path.parent().ok_or("CODEX_CONFIG_INVALID")?;
    fs::create_dir_all(parent).map_err(|_| "CODEX_CONFIG_WRITE_FAILED")?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let prefix = format!("hooks.json.codex-flow-{}-{stamp}", std::process::id());
    if let Some(bytes) = &original {
        let backup = parent.join(format!("{prefix}.backup"));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&backup)
            .map_err(|_| "CODEX_CONFIG_WRITE_FAILED")?;
        file.write_all(bytes)
            .and_then(|_| file.sync_all())
            .map_err(|_| "CODEX_CONFIG_WRITE_FAILED")?;
    }
    let temporary = parent.join(format!("{prefix}.tmp"));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|_| "CODEX_CONFIG_WRITE_FAILED")?;
        file.write_all(&updated)
            .and_then(|_| file.sync_all())
            .map_err(|_| "CODEX_CONFIG_WRITE_FAILED")?;
        drop(file);
        if read_existing(path)? != original {
            return Err("CODEX_CONFIG_CHANGED");
        }
        fs::rename(&temporary, path).map_err(|_| "CODEX_CONFIG_WRITE_FAILED")?;
        Ok("installed")
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

fn user_hook_path() -> Result<PathBuf, &'static str> {
    let base = std::env::var_os("CODEX_HOME")
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(|home| PathBuf::from(home).join(".codex")))
        .ok_or("CODEX_PATH_INVALID")?;
    if !base.is_absolute() {
        return Err("CODEX_PATH_INVALID");
    }
    Ok(base.join("hooks.json"))
}

#[tauri::command]
pub fn connect_codex_notifications(window: WebviewWindow) -> Result<&'static str, String> {
    if window.label() != "settings" {
        return Err("CODEX_SETUP_FORBIDDEN".into());
    }
    let exe = std::env::current_exe().map_err(|_| "CODEX_PATH_INVALID")?;
    let path = user_hook_path()?;
    install_hook(&path, &exe).map_err(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn adds_to_a_clean_profile_and_quotes_windows_paths() {
        let exe = Path::new(r"C:\Program Files\it's $ok\工具\codex-flow.exe");
        let bytes = hook_document(None, exe).unwrap().unwrap();
        let doc: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            doc["hooks"]["Stop"][0]["hooks"][0]["commandWindows"],
            "& 'C:\\Program Files\\it''s $ok\\工具\\codex-flow.exe' --codex-flow-stop-hook"
        );
        assert_eq!(doc["hooks"]["Stop"][0]["hooks"][0]["timeout"], 3);
    }
    #[test]
    fn preserves_other_hooks_and_is_idempotent() {
        let raw = br#"{"description":"keep","custom":{"x":1},"hooks":{"Stop":[{"hooks":[{"type":"command","command":"other"}]}],"SessionStart":[{"hooks":[]}]}}"#;
        let exe = Path::new(r"C:\Apps\codex-flow.exe");
        let updated = hook_document(Some(raw), exe).unwrap().unwrap();
        let doc: Value = serde_json::from_slice(&updated).unwrap();
        assert_eq!(doc["description"], "keep");
        assert_eq!(doc["custom"]["x"], 1);
        assert_eq!(doc["hooks"]["Stop"][0]["hooks"][0]["command"], "other");
        assert!(doc["hooks"]["SessionStart"].is_array());
        assert!(hook_document(Some(&updated), exe).unwrap().is_none());
    }
    #[test]
    fn refuses_invalid_configs_instead_of_overwriting_them() {
        let exe = Path::new(r"C:\Apps\codex-flow.exe");
        for raw in [
            b"bad".as_slice(),
            b"[]",
            br#"{"hooks":false}"#,
            br#"{"hooks":{"Stop":{}}}"#,
        ] {
            assert_eq!(
                hook_document(Some(raw), exe).unwrap_err(),
                "CODEX_CONFIG_INVALID"
            );
        }
    }

    #[test]
    fn malformed_file_is_not_changed_or_backed_up() {
        let root = std::env::temp_dir().join(format!(
            "codex-flow-invalid-hook-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("hooks.json");
        let original = b"not valid JSON";
        fs::write(&path, original).unwrap();
        assert_eq!(
            install_hook(&path, Path::new(r"C:\Apps\codex-flow.exe")),
            Err("CODEX_CONFIG_INVALID")
        );
        assert_eq!(fs::read(&path).unwrap(), original);
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
        fs::remove_file(path).unwrap();
        fs::remove_dir(root).unwrap();
    }
    #[test]
    fn installs_and_backs_up_only_in_an_isolated_directory() {
        let root = std::env::temp_dir().join(format!(
            "codex-flow-hook-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("hooks.json");
        let original = br#"{"hooks":{"Stop":[]},"extra":"keep"}"#;
        fs::write(&path, original).unwrap();
        assert_eq!(
            install_hook(&path, Path::new(r"C:\Apps\codex-flow.exe")).unwrap(),
            "installed"
        );
        assert_eq!(
            install_hook(&path, Path::new(r"C:\Apps\codex-flow.exe")).unwrap(),
            "existing"
        );
        let backups: Vec<_> = fs::read_dir(&root)
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "backup"))
            .collect();
        assert_eq!(backups.len(), 1);
        assert_eq!(fs::read(&backups[0]).unwrap(), original);
        fs::remove_file(&backups[0]).unwrap();
        fs::remove_file(&path).unwrap();
        fs::remove_dir(&root).unwrap();
    }
}
