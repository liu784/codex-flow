pub use crate::settings::{Settings, SettingsPatch};
use serde::Serialize;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex,
};
use tauri::{Emitter, Manager};
use tauri_plugin_autostart::ManagerExt;

pub struct DesktopState {
    settings: Mutex<Settings>,
    change: tokio::sync::Mutex<()>,
    pub utility_creation: tokio::sync::Mutex<()>,
    pub popup_dismissed: Mutex<Option<std::time::Instant>>,
    quitting: AtomicBool,
    orb_shown: AtomicBool,
    refresh_changed: tokio::sync::Notify,
}
impl Default for DesktopState {
    fn default() -> Self {
        Self {
            settings: Mutex::new(Settings::default()),
            change: Default::default(),
            utility_creation: Default::default(),
            popup_dismissed: Mutex::new(None),
            quitting: AtomicBool::new(false),
            orb_shown: AtomicBool::new(true),
            refresh_changed: Default::default(),
        }
    }
}
impl DesktopState {
    pub fn snapshot(&self) -> Result<Settings, String> {
        self.settings
            .lock()
            .map(|s| *s)
            .map_err(|_| "SETTINGS_LOCK_FAILED".into())
    }
    pub fn is_quitting(&self) -> bool {
        self.quitting.load(Ordering::SeqCst)
    }
    pub fn orb_shown(&self) -> bool {
        self.orb_shown.load(Ordering::SeqCst)
    }
}
fn settings_path(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    app.path()
        .app_local_data_dir()
        .map(|p| p.join("settings.json"))
        .map_err(|_| "SETTINGS_PATH_FAILED".into())
}
fn decode_settings(raw: &[u8]) -> Option<Settings> {
    serde_json::from_slice::<Settings>(raw)
        .ok()
        .and_then(|settings| settings.validate().ok())
}
fn persist(app: &tauri::AppHandle, next: Settings) -> Result<(), String> {
    let path = settings_path(app)?;
    std::fs::create_dir_all(path.parent().ok_or("SETTINGS_PATH_FAILED")?)
        .map_err(|_| "SETTINGS_SAVE_FAILED")?;
    let pending = path.with_extension("json.pending");
    std::fs::write(
        &pending,
        serde_json::to_vec(&next).map_err(|_| "SETTINGS_ENCODE_FAILED")?,
    )
    .map_err(|_| "SETTINGS_SAVE_FAILED")?;
    std::fs::rename(&pending, &path).map_err(|_| "SETTINGS_SAVE_FAILED".into())
}
#[tauri::command]
pub fn get_settings(
    app: tauri::AppHandle,
    state: tauri::State<'_, DesktopState>,
) -> Result<Settings, String> {
    let mut settings = state.snapshot()?;
    settings.auto_start = app
        .autolaunch()
        .is_enabled()
        .map_err(|_| "AUTOSTART_READ_FAILED")?;
    Ok(settings)
}
fn apply_runtime(app: &tauri::AppHandle, next: Settings) -> Result<(), String> {
    for label in ["main", "settings"] {
        if let Some(window) = app.get_webview_window(label) {
            window
                .set_always_on_top(next.always_on_top)
                .map_err(|_| "TOPMOST_APPLY_FAILED")?;
        }
    }
    if !next.main_panel_enabled {
        let _ = app.emit_to("main", "floating-collapse", ());
    }
    let native = app.state::<crate::native_orb::NativeOrbState>();
    native.configure_options(next.native_options());
    if native.dock_enabled() != next.orb_edge_dock_enabled {
        native.configure_dock(next.orb_edge_dock_enabled, None);
    }
    Ok(())
}
async fn commit_settings(
    app: &tauri::AppHandle,
    patch: Option<SettingsPatch>,
) -> Result<Settings, String> {
    let state = app.state::<DesktopState>();
    let _operation = state.change.lock().await;
    let mut previous = state.snapshot()?;
    previous.auto_start = app
        .autolaunch()
        .is_enabled()
        .map_err(|_| "AUTOSTART_READ_FAILED")?;
    let next = match patch {
        Some(p) => previous.patched(p)?,
        None => Settings::default(),
    };
    if next.auto_start != previous.auto_start {
        let result = if next.auto_start {
            app.autolaunch().enable()
        } else {
            app.autolaunch().disable()
        };
        result.map_err(|_| "AUTOSTART_APPLY_FAILED")?;
    }
    if let Err(error) = apply_runtime(app, next).and_then(|_| persist(app, next)) {
        let runtime_rollback = apply_runtime(app, previous);
        if next.auto_start != previous.auto_start {
            let rollback = if previous.auto_start {
                app.autolaunch().enable()
            } else {
                app.autolaunch().disable()
            };
            if rollback.is_err() {
                return Err("SETTINGS_ROLLBACK_FAILED".into());
            }
        }
        if runtime_rollback.is_err() {
            return Err("SETTINGS_ROLLBACK_FAILED".into());
        }
        return Err(error);
    }
    *state.settings.lock().map_err(|_| "SETTINGS_LOCK_FAILED")? = next;
    if next.auto_snap_top_center && !previous.auto_snap_top_center {
        crate::floating::snap_now(app).await;
    }
    if next.refresh_interval != previous.refresh_interval {
        state.refresh_changed.notify_one();
    }
    let _ = app.emit("settings-changed", next);
    Ok(next)
}
#[tauri::command]
pub async fn set_settings(app: tauri::AppHandle, patch: SettingsPatch) -> Result<Settings, String> {
    commit_settings(&app, Some(patch)).await
}
#[tauri::command]
pub async fn reset_settings(app: tauri::AppHandle, confirmed: bool) -> Result<Settings, String> {
    if !confirmed {
        return Err("RESET_CONFIRMATION_REQUIRED".into());
    }
    commit_settings(&app, None).await
}
#[tauri::command]
pub async fn open_settings(app: tauri::AppHandle) -> Result<(), String> {
    crate::utility_windows::open_settings(&app).await
}
#[tauri::command]
pub fn open_data_directory(app: tauri::AppHandle) -> Result<(), String> {
    let directory = app
        .path()
        .app_local_data_dir()
        .map_err(|_| "DATA_DIRECTORY_UNAVAILABLE")?;
    std::fs::create_dir_all(&directory).map_err(|_| "DATA_DIRECTORY_UNAVAILABLE")?;
    // Only a fixed app-owned directory; no frontend-provided command/path.
    std::process::Command::new("explorer.exe")
        .arg(directory)
        .spawn()
        .map(|_| ())
        .map_err(|_| "DATA_DIRECTORY_OPEN_FAILED".into())
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    version: String,
    runtime: String,
}
#[tauri::command]
pub fn get_app_info(app: tauri::AppHandle) -> AppInfo {
    AppInfo {
        version: app.package_info().version.to_string(),
        runtime: format!("Windows · Tauri {}", tauri::VERSION),
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrayState {
    orb_visible: bool,
    panel_expanded: bool,
}
#[tauri::command]
pub async fn get_tray_state(app: tauri::AppHandle) -> Result<TrayState, String> {
    let expanded = crate::floating::is_expanded(&app).await;
    Ok(TrayState {
        orb_visible: app.state::<DesktopState>().orb_shown() && !expanded,
        panel_expanded: expanded,
    })
}
#[tauri::command]
pub async fn tray_action(app: tauri::AppHandle, action: String) -> Result<(), String> {
    match action.as_str() {
        "show_panel" => {
            app.state::<DesktopState>()
                .orb_shown
                .store(true, Ordering::SeqCst);
            if crate::floating::is_expanded(&app).await {
                if let Some(w) = app.get_webview_window("main") {
                    w.show().map_err(|_| "PANEL_SHOW_FAILED")?;
                    w.set_focus().map_err(|_| "PANEL_FOCUS_FAILED")?;
                }
            } else {
                app.emit_to("main", "tray-open-panel", ())
                    .map_err(|_| "PANEL_OPEN_FAILED")?;
            }
        }
        "toggle_orb" => {
            let visible = get_tray_state(app.clone()).await?.orb_visible;
            app.state::<DesktopState>()
                .orb_shown
                .store(!visible, Ordering::SeqCst);
            if visible {
                crate::floating::hide_floating(&app)?;
            } else if crate::floating::is_expanded(&app).await {
                app.emit_to("main", "floating-collapse", ())
                    .map_err(|_| "ORB_SHOW_FAILED")?;
            } else {
                crate::floating::show_floating(&app).await?;
            }
        }
        "settings" => {
            crate::utility_windows::dismiss_popup(&app);
            open_settings(app.clone()).await?;
        }
        "exit" => quit(&app),
        _ => return Err("TRAY_ACTION_INVALID".into()),
    }
    let _ = app.emit("tray-state-changed", ());
    crate::utility_windows::dismiss_popup(&app);
    Ok(())
}
// Explicitly disabled for old acceptance clients; never reopen the retired warning window.
#[tauri::command]
pub fn get_quota_notification() -> Option<String> {
    None
}
#[tauri::command]
pub fn show_quota_notification_acceptance() -> Result<(), String> {
    Err("QUOTA_ALERT_WINDOW_RETIRED".into())
}

pub fn quit(app: &tauri::AppHandle) {
    if app
        .state::<DesktopState>()
        .quitting
        .swap(true, Ordering::SeqCst)
    {
        return;
    }
    let app = app.clone();
    app.state::<DesktopState>().refresh_changed.notify_one();
    tauri::async_runtime::spawn(async move {
        // Destroy all utility composition targets before shutting down the owned UI queue.
        let desktop = app.state::<DesktopState>();
        let _creation = desktop.utility_creation.lock().await;
        for label in ["tray-popup", "settings", "quota-alert"] {
            if let Some(w) = app.get_webview_window(label) {
                if w.destroy().is_err() {
                    app.state::<DesktopState>()
                        .quitting
                        .store(false, Ordering::SeqCst);
                    let _ = app.emit("exit-failed", "UTILITY_CLOSE_FAILED");
                    return;
                }
            }
        }
        if let Some(w) = app.get_webview_window("main") {
            let _ = crate::floating::save_floating_position(w.clone(), app.state()).await;
            if let Err(error) = crate::panel_backdrop::shutdown(&w).await {
                app.state::<DesktopState>()
                    .quitting
                    .store(false, Ordering::SeqCst);
                let _ = app.emit("exit-failed", error);
                return;
            }
        }
        app.state::<crate::core::AppCore>().shutdown().await;
        app.state::<crate::native_orb::NativeOrbState>().shutdown();
        app.remove_tray_by_id("codex-flow");
        app.exit(0);
    });
}
pub fn setup(app: &tauri::AppHandle) -> tauri::Result<()> {
    if let Ok(path) = settings_path(app) {
        if let Ok(raw) = std::fs::read(path) {
            if let Some(saved) = decode_settings(&raw) {
                if let Ok(mut settings) = app.state::<DesktopState>().settings.lock() {
                    *settings = saved;
                }
            }
        }
    }
    if let Ok(settings) = app.state::<DesktopState>().snapshot() {
        let _ = apply_runtime(app, settings);
    }
    crate::utility_windows::setup_tray(app)?;
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            let desktop = app.state::<DesktopState>();
            if desktop.is_quitting() {
                break;
            }
            let seconds = desktop.snapshot().map(|s| s.refresh_interval).unwrap_or(0);
            if seconds == 0 {
                desktop.refresh_changed.notified().await;
                continue;
            }
            tokio::select! {
                _ = desktop.refresh_changed.notified() => continue,
                _ = tokio::time::sleep(std::time::Duration::from_secs(u64::from(seconds))) => {}
            }
            if desktop.is_quitting() {
                break;
            }
            // Auto keeps the existing push stream; intervals reuse the existing refresh method.
            app.state::<crate::core::AppCore>().refresh().await;
        }
    });
    Ok(())
}

#[cfg(test)]
mod startup_tests {
    use super::*;

    #[test]
    fn corrupt_or_invalid_settings_keep_safe_defaults() {
        for raw in [
            b"not-json".as_slice(),
            br#"{"settingsSchemaVersion":99}"#.as_slice(),
            br#"{"refreshInterval":2}"#.as_slice(),
        ] {
            assert_eq!(
                decode_settings(raw).unwrap_or_default(),
                Settings::default()
            );
        }
    }
}
