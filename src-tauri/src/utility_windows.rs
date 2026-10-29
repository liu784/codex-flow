//! Settings and tray popup only. Both reuse the proven rounded backdrop lifecycle.
use crate::desktop::DesktopState;
use tauri::{Emitter, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

async fn window(
    app: &tauri::AppHandle,
    label: &str,
    width: f64,
    height: f64,
) -> Result<WebviewWindow, String> {
    let desktop = app.state::<DesktopState>();
    let _guard = desktop.utility_creation.lock().await;
    if desktop.is_quitting() {
        return Err("APP_QUITTING".into());
    }
    if let Some(w) = app.get_webview_window(label) {
        return Ok(w);
    }
    let w = WebviewWindowBuilder::new(
        app,
        label,
        WebviewUrl::App(format!("index.html?view={label}").into()),
    )
    .title(if label == "settings" {
        "Codex Flow — 设置"
    } else {
        "Codex Flow — 托盘菜单"
    })
    .inner_size(width, height)
    .decorations(false)
    .transparent(true)
    .shadow(false)
    .resizable(false)
    .skip_taskbar(true)
    .visible(false)
    .focused(false)
    .always_on_top(label == "tray-popup" || desktop.snapshot()?.always_on_top)
    .build()
    .map_err(|_| "UTILITY_WINDOW_CREATE_FAILED")?;
    if let Err(error) = crate::panel_backdrop::configure_utility(&w).await {
        let _ = w.destroy();
        return Err(error);
    }
    if label == "settings" {
        w.center().map_err(|_| "SETTINGS_CENTER_FAILED")?;
    }
    Ok(w)
}
pub async fn open_settings(app: &tauri::AppHandle) -> Result<(), String> {
    let w = window(app, "settings", 560.0, 390.0).await?;
    w.show().map_err(|_| "SETTINGS_SHOW_FAILED")?;
    w.set_focus().map_err(|_| "SETTINGS_FOCUS_FAILED")?;
    let _ = w.emit("utility-open", ());
    Ok(())
}
pub fn dismiss_popup(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("tray-popup") {
        if w.is_visible().unwrap_or(false) {
            let _ = w.hide();
            if let Ok(mut at) = app.state::<DesktopState>().popup_dismissed.lock() {
                *at = Some(std::time::Instant::now());
            }
        }
    }
}
#[tauri::command]
pub fn close_utility(window: WebviewWindow) -> Result<(), String> {
    if !matches!(window.label(), "settings" | "tray-popup") {
        return Err("UTILITY_WINDOW_REQUIRED".into());
    }
    window.hide().map_err(|_| "UTILITY_CLOSE_FAILED".into())
}
#[tauri::command]
pub fn drag_settings(window: WebviewWindow) -> Result<(), String> {
    if window.label() != "settings" {
        return Err("SETTINGS_WINDOW_REQUIRED".into());
    }
    window
        .start_dragging()
        .map_err(|_| "SETTINGS_DRAG_FAILED".into())
}

// Pure physical-coordinate placement: choose the free side of the tray and clamp to its monitor's work area.
pub fn popup_position(icon: [f64; 4], work: [f64; 4], size: [f64; 2], gap: f64) -> [i32; 2] {
    let [ix, iy, iw, ih] = icon;
    let [left, top, right, bottom] = work;
    let [width, height] = size;
    let (x, y) = if iy >= bottom - gap {
        (ix + iw - width, iy - height - gap)
    } else if iy + ih <= top + gap {
        (ix + iw - width, iy + ih + gap)
    } else if ix + iw <= left + gap {
        (ix + iw + gap, iy + ih - height)
    } else if ix >= right - gap {
        (ix - width - gap, iy + ih - height)
    } else {
        (ix + iw - width, iy - height - gap)
    };
    [
        x.clamp(left, (right - width).max(left)).round() as i32,
        y.clamp(top, (bottom - height).max(top)).round() as i32,
    ]
}
async fn toggle_popup(
    app: &tauri::AppHandle,
    point: tauri::PhysicalPosition<f64>,
    rect: tauri::Rect,
) -> Result<(), String> {
    if let Some(w) = app.get_webview_window("tray-popup") {
        if w.is_visible().unwrap_or(false) {
            dismiss_popup(app);
            return Ok(());
        }
    }
    // Clicking the icon first blurs the popup. Do not reopen it for that same click.
    if app
        .state::<DesktopState>()
        .popup_dismissed
        .lock()
        .map_err(|_| "POPUP_STATE_FAILED")?
        .is_some_and(|at| at.elapsed() < std::time::Duration::from_millis(200))
    {
        return Ok(());
    }
    let monitor = app
        .monitor_from_point(point.x, point.y)
        .map_err(|_| "TRAY_MONITOR_FAILED")?
        .ok_or("TRAY_MONITOR_FAILED")?;
    let scale = monitor.scale_factor();
    let position = rect.position.to_physical::<f64>(scale);
    let size = rect.size.to_physical::<f64>(scale);
    let area = monitor.work_area();
    let w = window(app, "tray-popup", 280.0, 332.0).await?;
    // Place before sizing so per-monitor DPI belongs to the actual tray monitor.
    let p = popup_position(
        [position.x, position.y, size.width, size.height],
        [
            area.position.x as f64,
            area.position.y as f64,
            (area.position.x + area.size.width as i32) as f64,
            (area.position.y + area.size.height as i32) as f64,
        ],
        [280.0 * scale, 332.0 * scale],
        4.0 * scale,
    );
    w.set_position(tauri::PhysicalPosition::new(p[0], p[1]))
        .map_err(|_| "TRAY_POSITION_FAILED")?;
    w.set_size(tauri::PhysicalSize::new(
        (280.0 * scale).round() as u32,
        (332.0 * scale).round() as u32,
    ))
    .map_err(|_| "TRAY_SIZE_FAILED")?;
    crate::panel_backdrop::configure_utility(&w).await?;
    w.show().map_err(|_| "TRAY_SHOW_FAILED")?;
    w.set_focus().map_err(|_| "TRAY_FOCUS_FAILED")?;
    let _ = w.emit("utility-open", ());
    Ok(())
}
pub fn setup_tray(app: &tauri::AppHandle) -> tauri::Result<()> {
    use tauri::{
        menu::{Menu, MenuItem},
        tray::{MouseButtonState, TrayIconBuilder, TrayIconEvent},
    };
    let panel = MenuItem::with_id(app, "show_panel", "显示主面板", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "设置", true, None::<&str>)?;
    let exit = MenuItem::with_id(app, "exit", "退出", true, None::<&str>)?;
    let fallback = Menu::with_items(app, &[&panel, &settings, &exit])?;
    let icon = tauri::image::Image::from_bytes(include_bytes!("../icons/tray-icon.png"))?;
    TrayIconBuilder::with_id("codex-flow")
        .icon(icon)
        .tooltip("Codex Flow")
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| {
            let app = app.clone();
            let action = event.id.as_ref().to_string();
            tauri::async_runtime::spawn(async move {
                let _ = crate::desktop::tray_action(app, action).await;
            });
        })
        .on_tray_icon_event(move |tray, event| {
            if let TrayIconEvent::Click {
                button_state: MouseButtonState::Up,
                position,
                rect,
                ..
            } = event
            {
                let tray = tray.clone();
                let fallback = fallback.clone();
                let app = tray.app_handle().clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(error) = toggle_popup(&app, position, rect).await {
                        let _ = tray.set_menu(Some(fallback));
                        let _ = tray.set_tooltip(Some("Codex Flow — 菜单恢复模式（右键）"));
                        let _ = app.emit("utility-error", error);
                    }
                });
            }
        })
        .build(app)?;
    app.state::<crate::startup::StartupMetrics>()
        .mark_tray_ready();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn taskbar_edges_and_negative_monitor_origin() {
        assert_eq!(
            popup_position(
                [1800., 1040., 24., 24.],
                [0., 0., 1920., 1040.],
                [280., 330.],
                4.
            ),
            [1544, 706]
        );
        assert_eq!(
            popup_position(
                [1800., 0., 24., 24.],
                [0., 40., 1920., 1080.],
                [280., 330.],
                4.
            ),
            [1544, 40]
        );
        assert_eq!(
            popup_position(
                [0., 900., 24., 24.],
                [40., 0., 1920., 1080.],
                [280., 330.],
                4.
            ),
            [40, 594]
        );
        assert_eq!(
            popup_position(
                [1880., 900., 24., 24.],
                [0., 0., 1880., 1080.],
                [280., 330.],
                4.
            ),
            [1596, 594]
        );
        let p = popup_position(
            [-100., 1040., 24., 24.],
            [-1920., 0., 0., 1040.],
            [560., 660.],
            8.,
        );
        assert_eq!(p, [-636, 372]);
    }
}
