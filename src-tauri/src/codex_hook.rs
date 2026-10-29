//! Receives only new Codex Stop-hook replies over a local, user-scoped pipe.
//! Neither the hook's transcript path nor other input fields are opened or saved.
use serde::Deserialize;
use std::io::{self, Read, Write};

const MAX_HOOK_INPUT: u64 = 1024 * 1024;
// The pipe carries the same reply as the hook input, encoded as a JSON string.
const MAX_PIPE_INPUT: u64 = MAX_HOOK_INPUT + 64;
const OVERSIZED_REPLY_NOTICE: &str = "Codex 回复过长，请打开 Codex 查看完整内容";

#[derive(Deserialize)]
struct StopInput {
    hook_event_name: String,
    last_assistant_message: Option<String>,
}

fn reply_preview(input: &[u8]) -> Option<String> {
    let event: StopInput = serde_json::from_slice(input).ok()?;
    if event.hook_event_name != "Stop" {
        return None;
    }
    let clean: String = event
        .last_assistant_message
        .unwrap_or_default()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .filter(|c| !c.is_control())
        .collect();
    Some(if clean.is_empty() {
        "有新回复".into()
    } else {
        clean
    })
}

#[cfg(windows)]
fn pipe_name() -> Option<String> {
    #[link(name = "advapi32")]
    unsafe extern "system" {
        fn GetUserNameW(buffer: *mut u16, size: *mut u32) -> i32;
    }
    let mut name = [0u16; 257];
    let mut length = name.len() as u32;
    if unsafe { GetUserNameW(name.as_mut_ptr(), &mut length) } == 0 || length < 2 {
        return None;
    }
    // Stable pipe suffix; both ends run under this Windows user.
    let mut hash = 0xcbf29ce484222325u64;
    for unit in &name[..(length as usize - 1)] {
        for byte in unit.to_le_bytes() {
            hash = (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3);
        }
    }
    Some(format!(r"\\.\pipe\CodexFlow-Stop-{hash:016x}"))
}

#[cfg(windows)]
fn same_process_user(peer: *mut std::ffi::c_void) -> bool {
    use std::{ffi::c_void, mem::size_of};
    #[repr(C)]
    struct SidAndAttributes { sid: *mut c_void, attributes: u32 }
    #[repr(C)]
    struct TokenUser { user: SidAndAttributes }
    #[link(name = "advapi32")]
    unsafe extern "system" {
        fn OpenProcessToken(process: *mut c_void, access: u32, token: *mut *mut c_void) -> i32;
        fn GetTokenInformation(token: *mut c_void, class: u32, info: *mut c_void, length: u32, needed: *mut u32) -> i32;
        fn EqualSid(left: *mut c_void, right: *mut c_void) -> i32;
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentProcess() -> *mut c_void;
        fn CloseHandle(handle: *mut c_void) -> i32;
    }
    fn token_user(process: *mut c_void) -> Option<Vec<usize>> {
        let mut token = std::ptr::null_mut();
        if unsafe { OpenProcessToken(process, 0x0008, &mut token) } == 0 { return None; }
        let mut length = 0;
        unsafe { GetTokenInformation(token, 1, std::ptr::null_mut(), 0, &mut length); }
        if length < size_of::<TokenUser>() as u32 || length > 65536 {
            unsafe { CloseHandle(token); }
            return None;
        }
        let mut buffer = vec![0usize; (length as usize + size_of::<usize>() - 1) / size_of::<usize>()];
        let ok = unsafe { GetTokenInformation(token, 1, buffer.as_mut_ptr().cast(), length, &mut length) } != 0;
        unsafe { CloseHandle(token); }
        ok.then_some(buffer)
    }
    let Some(ours_buffer) = token_user(unsafe { GetCurrentProcess() }) else { return false };
    let Some(theirs_buffer) = token_user(peer) else { return false };
    let ours = unsafe { &*(ours_buffer.as_ptr().cast::<TokenUser>()) }.user.sid;
    let theirs = unsafe { &*(theirs_buffer.as_ptr().cast::<TokenUser>()) }.user.sid;
    !ours.is_null() && !theirs.is_null() && unsafe { EqualSid(ours, theirs) } != 0
}

#[cfg(windows)]
fn expected_pipe_server(pipe: &std::fs::File, expected: &std::path::Path) -> bool {
    use std::ffi::c_void;
    use std::os::windows::{ffi::OsStringExt, io::AsRawHandle};
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetNamedPipeServerProcessId(pipe: *mut c_void, pid: *mut u32) -> i32;
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut c_void;
        fn QueryFullProcessImageNameW(
            process: *mut c_void,
            flags: u32,
            path: *mut u16,
            size: *mut u32,
        ) -> i32;
        fn CloseHandle(handle: *mut c_void) -> i32;
    }
    let mut pid = 0;
    if unsafe { GetNamedPipeServerProcessId(pipe.as_raw_handle(), &mut pid) } == 0 {
        return false;
    }
    let handle = unsafe { OpenProcess(0x1000, 0, pid) }; // PROCESS_QUERY_LIMITED_INFORMATION
    if handle.is_null() {
        return false;
    }
    if !same_process_user(handle) {
        unsafe { CloseHandle(handle); }
        return false;
    }
    let mut path = vec![0u16; 32768];
    let mut length = path.len() as u32;
    let found =
        unsafe { QueryFullProcessImageNameW(handle, 0, path.as_mut_ptr(), &mut length) } != 0;
    unsafe {
        CloseHandle(handle);
    }
    if !found {
        return false;
    }
    let peer = std::path::PathBuf::from(std::ffi::OsString::from_wide(&path[..length as usize]));
    match (std::fs::canonicalize(peer), std::fs::canonicalize(expected)) {
        (Ok(peer), Ok(current)) => peer
            .to_string_lossy()
            .eq_ignore_ascii_case(&current.to_string_lossy()),
        _ => false,
    }
}

#[cfg(windows)]
fn open_pipe_with_retry(name: &str) -> io::Result<std::fs::File> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
    loop {
        match std::fs::OpenOptions::new().write(true).open(name) {
            Ok(pipe) => return Ok(pipe),
            Err(error) if matches!(error.raw_os_error(), Some(2) | Some(231))
                && std::time::Instant::now() < deadline =>
            {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            Err(error) => return Err(error),
        }
    }
}

#[cfg(windows)]
fn new_pipe_server(name: &str) -> io::Result<tokio::net::windows::named_pipe::NamedPipeServer> {
    use tokio::net::windows::named_pipe::ServerOptions;
    ServerOptions::new()
        .access_inbound(true)
        .access_outbound(false)
        .reject_remote_clients(true)
        .first_pipe_instance(true)
        .max_instances(1)
        .create(name)
}

/// Called only by Codex's explicit Stop hook. Always returns a non-blocking `{}`.
pub fn run_stop_hook() {
    let mut input = Vec::new();
    let mut stdin = io::stdin().lock();
    let read = (&mut stdin)
        .take(MAX_HOOK_INPUT + 1)
        .read_to_end(&mut input);
    let reply = if input.len() as u64 > MAX_HOOK_INPUT {
        // Drain the rest so Codex's writer is not left with a broken pipe.
        let _ = io::copy(&mut stdin, &mut io::sink());
        Some(OVERSIZED_REPLY_NOTICE.to_owned())
    } else if read.is_ok() {
        reply_preview(&input)
    } else {
        None
    };
    if let Some(text) = reply {
        #[cfg(windows)]
        if let Some(name) = pipe_name() {
            let opened = open_pipe_with_retry(&name);
            if let Ok(mut pipe) = opened {
                let expected = std::env::args_os()
                    .nth(2)
                    .map(std::path::PathBuf::from)
                    .or_else(|| std::env::current_exe().ok());
                if !expected
                    .as_deref()
                    .is_some_and(|path| expected_pipe_server(&pipe, path))
                {
                    let _ = writeln!(io::stdout(), "{{}}");
                    return;
                }
                if let Ok(bytes) = serde_json::to_vec(&text) {
                    if bytes.len() as u64 <= MAX_PIPE_INPUT {
                        let _ = pipe.write_all(&bytes);
                    }
                }
            }
        }
    }
    let _ = writeln!(io::stdout(), "{{}}");
}

#[cfg(windows)]
pub(crate) fn start_listener(app: tauri::AppHandle) {
    use std::time::Duration;
    use tauri::Manager;
    use tokio::io::AsyncReadExt;

    let Some(name) = pipe_name() else { return };
    tauri::async_runtime::spawn(async move {
        // Inbound-only, first instance, and the Windows default user/owner DACL.
        // Remote clients are rejected; no message data is written to disk.
        loop {
            if app.state::<crate::desktop::DesktopState>().is_quitting() {
                return;
            }
            // A used Tokio pipe can retain EOF readiness after disconnecting.
            // Create a fresh instance for every message instead of reusing it.
            let mut pipe = match new_pipe_server(&name) {
                Ok(pipe) => pipe,
                Err(_) => {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    continue;
                }
            };
            let connected = loop {
                tokio::select! {
                    result = pipe.connect() => break result.is_ok(),
                    _ = tokio::time::sleep(Duration::from_secs(1)) => {
                        if app.state::<crate::desktop::DesktopState>().is_quitting() { return; }
                    },
                }
            };
            if !connected {
                continue;
            }
            let mut bytes = Vec::new();
            let read = tokio::time::timeout(Duration::from_secs(2), async {
                (&mut pipe)
                    .take(MAX_PIPE_INPUT + 1)
                    .read_to_end(&mut bytes)
                    .await
            })
            .await;
            drop(pipe);
            if matches!(read, Ok(Ok(_))) && (bytes.len() as u64) <= MAX_PIPE_INPUT {
                if let Ok(text) = serde_json::from_slice::<String>(&bytes) {
                    let clean: String = text
                        .chars()
                        .filter(|c| !c.is_control())
                        .collect();
                    if !clean.trim().is_empty() {
                        crate::island_notifications::emit_codex_reply(&app, clean);
                    }
                }
            }
        }
    });
}

#[cfg(not(windows))]
pub(crate) fn start_listener(_app: tauri::AppHandle) {}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(windows)]
    #[test]
    fn current_process_has_matching_user_identity() {
        #[link(name = "kernel32")]
        unsafe extern "system" { fn GetCurrentProcess() -> *mut std::ffi::c_void; }
        assert!(same_process_user(unsafe { GetCurrentProcess() }));
    }
    #[test]
    fn accepts_only_stop_replies_and_keeps_the_complete_text() {
        let input = br#"{"hook_event_name":"Stop","last_assistant_message":"A\n  B","prompt":"SECRET","transcript_path":"not-opened"}"#;
        assert_eq!(reply_preview(input).as_deref(), Some("A B"));
        assert_eq!(
            reply_preview(
                br#"{"hook_event_name":"UserPromptSubmit","last_assistant_message":"ignore"}"#
            ),
            None
        );
        assert_eq!(
            reply_preview(br#"{"hook_event_name":"Stop","last_assistant_message":null}"#)
                .as_deref(),
            Some("有新回复")
        );
        let expected = format!("开头 {} 结尾 🐱", "猫\"\\".repeat(4000));
        let input = serde_json::json!({"hook_event_name":"Stop","last_assistant_message":expected}).to_string();
        assert_eq!(reply_preview(input.as_bytes()).as_deref(), Some(expected.as_str()));
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn local_named_pipe_transfers_a_long_unicode_reply() {
        use tokio::io::AsyncReadExt;
        use tokio::net::windows::named_pipe::ServerOptions;
        let name = format!(r"\\.\pipe\CodexFlow-Test-{}", std::process::id());
        let mut server = ServerOptions::new()
            .access_outbound(false)
            .reject_remote_clients(true)
            .first_pipe_instance(true)
            .max_instances(1)
            .create(&name)
            .unwrap();
        let expected = format!("开头 {} 结尾 🐱", "猫 and text ".repeat(6000));
        let sent = expected.clone();
        let sender = std::thread::spawn(move || {
            let mut client = std::fs::OpenOptions::new().write(true).open(name).unwrap();
            assert!(
                expected_pipe_server(&client, &std::env::current_exe().unwrap()),
                "only the same executable may receive a reply"
            );
            client.write_all(&serde_json::to_vec(&sent).unwrap()).unwrap();
        });
        server.connect().await.unwrap();
        let mut bytes = Vec::new();
        server.read_to_end(&mut bytes).await.unwrap();
        sender.join().unwrap();
        assert_eq!(
            serde_json::from_slice::<String>(&bytes).unwrap(),
            expected
        );
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "requires the running registered Codex Flow preview"]
    fn live_preview_pipe_authenticates_without_reading_messages() {
        let name = pipe_name().expect("current user's pipe name");
        let pipe = std::fs::OpenOptions::new().write(true).open(name).expect("open preview pipe");
        let expected = std::env::var_os("CODEX_FLOW_TEST_SERVER").expect("preview path");
        assert!(expected_pipe_server(&pipe, std::path::Path::new(&expected)));
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn sequential_clients_can_send_after_server_identity_checks() {
        use tokio::io::AsyncReadExt;
        let name = format!(r"\\.\pipe\CodexFlow-Sequence-Test-{}", std::process::id());
        let client_name = name.clone();
        let sender = std::thread::spawn(move || -> io::Result<()> {
            for index in 0..3 {
                let mut client = open_pipe_with_retry(&client_name)?;
                assert!(expected_pipe_server(&client, &std::env::current_exe().unwrap()));
                // The real sender authenticates the peer before writing.
                std::thread::sleep(std::time::Duration::from_millis(20));
                client.write_all(format!("message-{index}").as_bytes())?;
                drop(client);
            }
            Ok(())
        });
        let mut received = Vec::new();
        for _ in 0..3 {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
            let mut server = loop {
                match new_pipe_server(&name) {
                    Ok(server) => break server,
                    Err(_) if std::time::Instant::now() < deadline => {
                        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                    }
                    Err(error) => panic!("could not create the next pipe: {error}"),
                }
            };
            let connected = tokio::time::timeout(std::time::Duration::from_secs(2), server.connect()).await;
            if !matches!(connected, Ok(Ok(()))) { break; }
            let mut bytes = Vec::new();
            let read = tokio::time::timeout(std::time::Duration::from_secs(2), server.read_to_end(&mut bytes)).await;
            drop(server);
            if !matches!(read, Ok(Ok(_))) { break; }
            received.push(String::from_utf8(bytes).unwrap());
            // A sender can arrive while the next instance is being created.
            tokio::time::sleep(std::time::Duration::from_millis(120)).await;
        }
        let result = sender.join().unwrap();
        assert!(result.is_ok(), "a client was disconnected before writing: {result:?}");
        assert_eq!(received, ["message-0", "message-1", "message-2"]);
    }
}
