//! Local Codex and Grok Bot replies. No Windows notification access is requested.
use serde::Serialize;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
    time::Duration,
};
use tauri::{Emitter, Manager, WebviewWindow};

#[derive(Default)]
pub struct NotificationState {
    enabled: AtomicBool,
    emission: Mutex<Vec<(&'static str, String, std::time::Instant)>>,
}

#[derive(Clone, Serialize)]
struct IslandMessage {
    source: &'static str,
    text: String,
}
#[derive(Serialize)]
pub struct NotificationStatus {
    enabled: bool,
}

pub(crate) fn preview(first: &str, second: &str) -> String {
    fn clean(value: &str) -> String {
        value.split_whitespace().collect::<Vec<_>>().join(" ")
    }
    let first = clean(first);
    let second = clean(second);
    let second = if !first.is_empty() {
        second
            .strip_prefix(&format!("{first}："))
            .or_else(|| second.strip_prefix(&format!("{first}:")))
            .map(|s| s.trim().to_owned())
            .unwrap_or(second)
    } else {
        second
    };
    let joined = match (first.is_empty(), second.is_empty(), first == second) {
        (true, true, _) => "新通知".to_owned(),
        (true, false, _) => second,
        (false, true, _) | (false, false, true) => first,
        _ => format!("{first}：{second}"),
    };
    joined
        .chars()
        .filter(|ch| !ch.is_control())
        .take(500)
        .collect()
}

fn should_emit_deduped(
    recent: &mut Vec<(&'static str, String, std::time::Instant)>,
    source: &'static str,
    text: &str,
    now: std::time::Instant,
) -> bool {
    recent.retain(|(_, _, prev_at)| {
        now.saturating_duration_since(*prev_at) < Duration::from_secs(15)
    });
    if recent
        .iter()
        .any(|(prev_src, prev_text, _)| *prev_src == source && prev_text == text)
    {
        return false;
    }
    recent.push((source, text.to_owned(), now));
    true
}
fn emit_message(app: &tauri::AppHandle, source: &'static str, text: String) {
    let state = app.state::<NotificationState>();
    if let Ok(mut guard) = state.emission.lock() {
        if state.enabled.load(Ordering::SeqCst)
            && should_emit_deduped(&mut guard, source, &text, std::time::Instant::now())
        {
            let _ = app.emit_to("main", "island-message", IslandMessage { source, text });
        }
    };
}
pub(crate) fn emit_codex_reply(app: &tauri::AppHandle, text: String) {
    emit_message(app, "Codex", text);
}
pub(crate) fn emit_grok_reply(app: &tauri::AppHandle, text: String) {
    emit_message(app, "Grok Bot", text);
}
pub(crate) fn messages_enabled(app: &tauri::AppHandle) -> bool {
    app.state::<NotificationState>()
        .enabled
        .load(Ordering::SeqCst)
}

fn preference_path(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    app.path()
        .app_local_data_dir()
        .map(|path| path.join("island-notifications.json"))
        .map_err(|_| "NOTIFICATION_PATH_FAILED".into())
}
fn save_preference(app: &tauri::AppHandle, enabled: bool) -> Result<(), String> {
    let path = preference_path(app)?;
    std::fs::create_dir_all(path.parent().ok_or("NOTIFICATION_PATH_FAILED")?)
        .map_err(|_| "NOTIFICATION_SAVE_FAILED")?;
    let pending = path.with_extension("json.pending");
    std::fs::write(
        &pending,
        if enabled {
            b"true".as_slice()
        } else {
            b"false".as_slice()
        },
    )
    .map_err(|_| "NOTIFICATION_SAVE_FAILED")?;
    std::fs::rename(pending, path).map_err(|_| "NOTIFICATION_SAVE_FAILED".into())
}
fn status(app: &tauri::AppHandle) -> NotificationStatus {
    NotificationStatus {
        enabled: app
            .state::<NotificationState>()
            .enabled
            .load(Ordering::SeqCst),
    }
}
fn require_settings(window: &WebviewWindow) -> Result<(), String> {
    if window.label() == "settings" {
        Ok(())
    } else {
        Err("NOTIFICATION_FORBIDDEN".into())
    }
}
#[tauri::command]
pub fn get_island_notification_status(
    app: tauri::AppHandle,
    window: WebviewWindow,
) -> Result<NotificationStatus, String> {
    require_settings(&window)?;
    Ok(status(&app))
}
#[tauri::command]
pub fn enable_island_notifications(
    app: tauri::AppHandle,
    window: WebviewWindow,
) -> Result<NotificationStatus, String> {
    require_settings(&window)?;
    save_preference(&app, true)?;
    app.state::<NotificationState>()
        .enabled
        .store(true, Ordering::SeqCst);
    Ok(status(&app))
}
#[tauri::command]
pub fn disable_island_notifications(
    app: tauri::AppHandle,
    window: WebviewWindow,
) -> Result<(), String> {
    require_settings(&window)?;
    let state = app.state::<NotificationState>();
    let mut guard = state
        .emission
        .lock()
        .map_err(|_| "NOTIFICATION_STATE_FAILED")?;
    guard.clear();
    save_preference(&app, false)?;
    state.enabled.store(false, Ordering::SeqCst);
    let _ = app.emit_to("main", "island-message-clear", ());
    Ok(())
}
fn notification_preference(raw: Option<&[u8]>) -> bool {
    raw.and_then(|bytes| serde_json::from_slice::<bool>(bytes).ok())
        .unwrap_or(false)
}
pub fn setup(app: &tauri::AppHandle) {
    let raw = preference_path(app)
        .ok()
        .and_then(|path| std::fs::read(path).ok());
    app.state::<NotificationState>()
        .enabled
        .store(notification_preference(raw.as_deref()), Ordering::SeqCst);
    crate::codex_hook::start_listener(app.clone());
    crate::grok_messages::start_listener(app.clone());
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reminders_require_an_explicit_valid_enabled_preference() {
        for raw in [
            None,
            Some(b"broken".as_slice()),
            Some(b"false"),
            Some(b"{}"),
        ] {
            assert!(!notification_preference(raw));
        }
        assert!(notification_preference(Some(b"true")));
    }
    #[test]
    fn previews_are_bounded_clean_and_do_not_duplicate_text() {
        assert_eq!(preview("小明\n", " 你好\t世界 "), "小明：你好 世界");
        assert_eq!(preview("小明", "小明：你好"), "小明：你好");
        assert_eq!(preview("小明", "小明: 你好"), "小明：你好");
        assert_eq!(preview("同一条", "同一条"), "同一条");
        assert_eq!(preview("", ""), "新通知");
        assert_eq!(preview(&"猫".repeat(600), "").chars().count(), 500);
        assert!(!preview("abc\u{0000}", "def").contains('\0'));
    }
    #[test]
    fn duplicate_emissions_within_window_are_suppressed() {
        let mut recent = Vec::new();
        let t0 = std::time::Instant::now();
        assert!(should_emit_deduped(&mut recent, "Grok Bot", "一", t0));
        assert!(!should_emit_deduped(
            &mut recent,
            "Grok Bot",
            "一",
            t0 + Duration::from_secs(2)
        ));
        assert!(should_emit_deduped(
            &mut recent,
            "Grok Bot",
            "二",
            t0 + Duration::from_secs(2)
        ));
        assert!(should_emit_deduped(
            &mut recent,
            "Grok Bot",
            "一",
            t0 + Duration::from_secs(16)
        ));
    }
}
