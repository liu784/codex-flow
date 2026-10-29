use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::{AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition, WebviewWindow};
use tokio::sync::Mutex;

use crate::native_orb::NativeOrbState;

const COLLAPSED_WINDOW_WIDTH: f64 = 280.0;
const COLLAPSED_WINDOW_HEIGHT: f64 = 62.0;
const MUSIC_WINDOW_WIDTH: f64 = 440.0;
const EXPANDED_WINDOW_WIDTH: f64 = 760.0;
const EXPANDED_WINDOW_HEIGHT: f64 = 470.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SavedPosition {
    pub version: u8,
    pub x: i32,
    pub y: i32,
    #[serde(default, skip_serializing_if = "is_free")]
    pub dock_edge: u8,
}
fn is_free(edge: &u8) -> bool {
    *edge == 0
}

#[derive(Clone, Copy)]
pub struct WorkArea {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub scale: f64,
}

pub fn restore_position(saved: SavedPosition, areas: &[WorkArea], expanded: bool) -> SavedPosition {
    let closest = areas.iter().min_by_key(|area| {
        // A docked full window may have its 12px margin on the adjacent screen.
        // Select by the ball center, not that outlying window corner.
        let center_offset = if saved.dock_edge != 0 {
            (52.0 * area.scale).round() as i64
        } else {
            0
        };
        let center_x = i64::from(saved.x) + center_offset;
        let center_y = i64::from(saved.y) + center_offset;
        let x = center_x.clamp(i64::from(area.x), i64::from(area.x) + i64::from(area.width));
        let y = center_y.clamp(
            i64::from(area.y),
            i64::from(area.y) + i64::from(area.height),
        );
        (center_x - x).unsigned_abs() + (center_y - y).unsigned_abs()
    });
    let Some(area) = closest else { return saved };
    let (width, height) = if expanded {
        (EXPANDED_WINDOW_WIDTH, EXPANDED_WINDOW_HEIGHT)
    } else {
        (COLLAPSED_WINDOW_WIDTH, COLLAPSED_WINDOW_HEIGHT)
    };
    let clamp = |value: i32, origin: i32, extent: u32, size: f64| {
        let low = i64::from(origin);
        let high = (low + i64::from(extent) - (size * area.scale).ceil() as i64).max(low);
        i64::from(value).clamp(low, high) as i32
    };
    SavedPosition {
        version: 1,
        dock_edge: saved.dock_edge,
        x: clamp(saved.x, area.x, area.width, width),
        y: clamp(saved.y, area.y, area.height, height),
    }
}

fn default_island_position(area: WorkArea) -> SavedPosition {
    let width = (COLLAPSED_WINDOW_WIDTH * area.scale).ceil() as i64;
    let left = i64::from(area.x) + ((i64::from(area.width) - width).max(0) / 2);
    let top = i64::from(area.y) + (8.0 * area.scale).round() as i64;
    restore_position(
        SavedPosition {
            version: 1,
            x: left as i32,
            y: top as i32,
            dock_edge: 0,
        },
        &[area],
        false,
    )
}

fn panel_position(anchor: SavedPosition, areas: &[WorkArea]) -> SavedPosition {
    let closest = areas.iter().min_by_key(|area| {
        let cx = i64::from(anchor.x) + (COLLAPSED_WINDOW_WIDTH * area.scale / 2.0).round() as i64;
        let cy = i64::from(anchor.y) + (COLLAPSED_WINDOW_HEIGHT * area.scale / 2.0).round() as i64;
        let x = cx.clamp(i64::from(area.x), i64::from(area.x) + i64::from(area.width));
        let y = cy.clamp(
            i64::from(area.y),
            i64::from(area.y) + i64::from(area.height),
        );
        (cx - x).unsigned_abs() + (cy - y).unsigned_abs()
    });
    let Some(area) = closest else { return anchor };
    let shift =
        ((EXPANDED_WINDOW_WIDTH - COLLAPSED_WINDOW_WIDTH) * area.scale / 2.0).round() as i32;
    let panel = SavedPosition {
        x: anchor.x.saturating_sub(shift),
        ..anchor
    };
    restore_position(panel, &[*area], true)
}

const TOP_SNAP_OFFSET_LOGICAL: f64 = -7.0;

fn top_center_island_position(area: WorkArea) -> SavedPosition {
    let width = (COLLAPSED_WINDOW_WIDTH * area.scale).ceil() as i64;
    let left = i64::from(area.x) + ((i64::from(area.width) - width).max(0) / 2);
    let top = i64::from(area.y) + (TOP_SNAP_OFFSET_LOGICAL * area.scale).round() as i64;
    SavedPosition {
        version: 1,
        x: left as i32,
        y: top as i32,
        dock_edge: 0,
    }
}

fn snap_top_center(reference: SavedPosition, areas: &[WorkArea]) -> SavedPosition {
    let closest = areas.iter().min_by_key(|area| {
        let cx = i64::from(reference.x) + (COLLAPSED_WINDOW_WIDTH * area.scale / 2.0).round() as i64;
        let cy = i64::from(reference.y) + (COLLAPSED_WINDOW_HEIGHT * area.scale / 2.0).round() as i64;
        let x = cx.clamp(i64::from(area.x), i64::from(area.x) + i64::from(area.width));
        let y = cy.clamp(i64::from(area.y), i64::from(area.y) + i64::from(area.height));
        (cx - x).unsigned_abs() + (cy - y).unsigned_abs()
    });
    match closest {
        Some(area) => top_center_island_position(*area),
        None => reference,
    }
}

fn music_position(anchor: SavedPosition, areas: &[WorkArea]) -> SavedPosition {
    let closest = areas.iter().min_by_key(|area| {
        let cx = i64::from(anchor.x) + (COLLAPSED_WINDOW_WIDTH * area.scale / 2.0).round() as i64;
        let cy = i64::from(anchor.y) + (COLLAPSED_WINDOW_HEIGHT * area.scale / 2.0).round() as i64;
        let x = cx.clamp(i64::from(area.x), i64::from(area.x) + i64::from(area.width));
        let y = cy.clamp(i64::from(area.y), i64::from(area.y) + i64::from(area.height));
        (cx - x).unsigned_abs() + (cy - y).unsigned_abs()
    });
    let Some(area) = closest else { return anchor };
    let shift = ((MUSIC_WINDOW_WIDTH - COLLAPSED_WINDOW_WIDTH) * area.scale / 2.0).round() as i64;
    let left = i64::from(area.x);
    let right = (left + i64::from(area.width) - (MUSIC_WINDOW_WIDTH * area.scale).ceil() as i64).max(left);
    let top = i64::from(area.y) + (TOP_SNAP_OFFSET_LOGICAL * area.scale).round() as i64;
    let bottom = (i64::from(area.y) + i64::from(area.height) - (COLLAPSED_WINDOW_HEIGHT * area.scale).ceil() as i64).max(top);
    SavedPosition {
        x: (i64::from(anchor.x) - shift).clamp(left, right) as i32,
        y: i64::from(anchor.y).clamp(top, bottom) as i32,
        ..anchor
    }
}

pub fn decode_position(raw: &str) -> Option<SavedPosition> {
    serde_json::from_str::<SavedPosition>(raw)
        .ok()
        .filter(|p| p.version == 1 && p.dock_edge <= 2)
}

#[derive(Default)]
pub struct Geometry {
    anchor: Option<SavedPosition>,
    origin: Option<SavedPosition>,
    expanded: bool,
    music_wide: bool,
    panel_acrylic: bool,
}
#[derive(Default)]
pub struct FloatingState(pub Mutex<Geometry>);

pub async fn is_expanded(app: &AppHandle) -> bool {
    app.state::<FloatingState>().0.lock().await.expanded
}

fn areas(window: &WebviewWindow) -> Result<Vec<WorkArea>, String> {
    window
        .available_monitors()
        .map(|monitors| {
            monitors
                .iter()
                .map(|monitor| {
                    let area = monitor.work_area();
                    WorkArea {
                        x: area.position.x,
                        y: area.position.y,
                        width: area.size.width,
                        height: area.size.height,
                        scale: monitor.scale_factor(),
                    }
                })
                .collect()
        })
        .map_err(|_| "MONITOR_UNAVAILABLE".into())
}
fn current(window: &WebviewWindow) -> Result<SavedPosition, String> {
    window
        .outer_position()
        .map(|p| SavedPosition {
            version: 1,
            dock_edge: 0,
            x: p.x,
            y: p.y,
        })
        .map_err(|_| "POSITION_UNAVAILABLE".into())
}
fn path(window: &WebviewWindow) -> Result<PathBuf, String> {
    // Gate 3 stores coordinates only, in the application's own local data directory.
    window
        .app_handle()
        .path()
        .app_local_data_dir()
        // The old position belongs to the retired ball; do not move or overwrite it.
        .map(|p| p.join("island-position.json"))
        .map_err(|_| "POSITION_STORAGE_UNAVAILABLE".into())
}
fn write_position(window: &WebviewWindow, position: SavedPosition) -> Result<(), String> {
    let path = path(window)?;
    std::fs::create_dir_all(path.parent().ok_or("POSITION_STORAGE_UNAVAILABLE")?)
        .map_err(|_| "POSITION_STORAGE_UNAVAILABLE")?;
    let data = serde_json::to_vec(&position).map_err(|_| "POSITION_STORAGE_UNAVAILABLE")?;
    std::fs::write(path, data).map_err(|_| "POSITION_STORAGE_UNAVAILABLE".into())
}
fn track_drag(window: &WebviewWindow, geometry: &mut Geometry) -> Result<(), String> {
    let position = current(window)?;
    if let (Some(anchor), Some(origin)) = (geometry.anchor, geometry.origin) {
        geometry.anchor = Some(SavedPosition {
            version: 1,
            dock_edge: anchor.dock_edge,
            x: anchor.x.saturating_add(position.x.saturating_sub(origin.x)),
            y: anchor.y.saturating_add(position.y.saturating_sub(origin.y)),
        });
    }
    geometry.origin = Some(position);
    Ok(())
}

#[tauri::command]
pub async fn restore_floating_window(
    window: WebviewWindow,
    state: tauri::State<'_, FloatingState>,
    native: tauri::State<'_, NativeOrbState>,
) -> Result<(), String> {
    let mut geometry = state.0.lock().await;
    if geometry.anchor.is_some() {
        return Ok(());
    }
    let restored = (|| -> Result<SavedPosition, String> {
        let saved = path(&window)
            .ok()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|raw| decode_position(&raw));
        let areas = areas(&window)?;
        let primary = window.primary_monitor().ok().flatten().map(|monitor| {
            let area = monitor.work_area();
            WorkArea {
                x: area.position.x,
                y: area.position.y,
                width: area.size.width,
                height: area.size.height,
                scale: monitor.scale_factor(),
            }
        });
        let initial = match primary.or_else(|| areas.first().copied()) {
            Some(area) => default_island_position(area),
            None => current(&window)?,
        };
        let auto_snap = window
            .app_handle()
            .state::<crate::desktop::DesktopState>()
            .snapshot()
            .map(|s| s.auto_snap_top_center)
            .unwrap_or(true);
        let anchor = if auto_snap {
            match primary.or_else(|| areas.first().copied()) {
                Some(area) => top_center_island_position(area),
                None => snap_top_center(saved.unwrap_or(initial), &areas),
            }
        } else {
            saved.unwrap_or(initial)
        };
        let position = music_position(anchor, &areas);
        let _ = write_position(&window, anchor);
        window
            .set_size(LogicalSize::new(MUSIC_WINDOW_WIDTH, COLLAPSED_WINDOW_HEIGHT))
            .map_err(|_| "WINDOW_RESIZE_FAILED")?;
        window
            .set_position(PhysicalPosition::new(position.x, position.y))
            .map_err(|_| "POSITION_FAILED")?;
        geometry.anchor = Some(anchor);
        geometry.origin = Some(position);
        Ok(position)
    })();
    drop(geometry);
    let position = match restored {
        Ok(position) => position,
        Err(error) => {
            window.show().map_err(|_| "WINDOW_SHOW_FAILED")?;
            crate::startup::mark_orb_ready(window.app_handle());
            return Err(error);
        }
    };
    let status = native.ensure_started(position.x, position.y);
    let result = if status.active {
        native.set_position(position.x, position.y);
        native.configure_dock(native.dock_enabled(), Some(position.dock_edge));
        native.set_visible(
            window
                .app_handle()
                .state::<crate::desktop::DesktopState>()
                .orb_shown(),
        );
        window.hide().map_err(|_| "WINDOW_HIDE_FAILED".into())
    } else {
        // Every native initialization failure is a renderer fallback, not an app failure.
        window.show().map_err(|_| "WINDOW_SHOW_FAILED".into())
    };
    if result.is_ok() {
        crate::startup::mark_orb_ready(window.app_handle());
    }
    result
}

#[tauri::command]
pub async fn set_floating_music_wide(
    window: WebviewWindow,
    state: tauri::State<'_, FloatingState>,
    wide: bool,
) -> Result<(), String> {
    let mut geometry = state.0.lock().await;
    track_drag(&window, &mut geometry)?;
    if geometry.music_wide == wide { return Ok(()); }
    geometry.music_wide = wide;
    Ok(())
}

#[tauri::command]
pub async fn set_floating_expanded(
    window: WebviewWindow,
    state: tauri::State<'_, FloatingState>,
    native: tauri::State<'_, NativeOrbState>,
    expanded: bool,
) -> Result<bool, String> {
    let mut geometry = state.0.lock().await;
    track_drag(&window, &mut geometry)?;
    if geometry.expanded == expanded {
        return Ok(geometry.panel_acrylic);
    }
    let native_active = native.status().active;
    if native_active {
        if let Some(position) = native.dock_position() {
            if !geometry.expanded
                || position.dock_edge != 0
                || geometry.anchor.is_some_and(|p| p.dock_edge != 0)
            {
                geometry.anchor = Some(position);
            }
        }
    }
    if expanded && native_active {
        native.set_visible(false);
    }
    let before = current(&window)?;
    let anchor = geometry.anchor.unwrap_or(before);
    let work_areas = areas(&window)?;
    let position = if expanded {
        panel_position(anchor, &work_areas)
    } else {
        music_position(anchor, &work_areas)
    };
    let (width, height) = if expanded {
        (EXPANDED_WINDOW_WIDTH, EXPANDED_WINDOW_HEIGHT)
    } else {
        (MUSIC_WINDOW_WIDTH, COLLAPSED_WINDOW_HEIGHT)
    };
    // Remove expanded-only effects before the HWND can host the legacy orb.
    let was_acrylic = geometry.panel_acrylic;
    if was_acrylic {
        crate::panel_backdrop::configure(&window, false).await?;
        geometry.panel_acrylic = false;
    }
    if window.set_size(LogicalSize::new(width, height)).is_err() {
        if was_acrylic {
            geometry.panel_acrylic = crate::panel_backdrop::configure(&window, true).await?;
        }
        if expanded && native_active {
            native.set_visible(true);
        }
        return Err("WINDOW_RESIZE_FAILED".into());
    }
    if let Err(_error) = window.set_position(PhysicalPosition::new(position.x, position.y)) {
        let (w, h) = if geometry.expanded {
            (EXPANDED_WINDOW_WIDTH, EXPANDED_WINDOW_HEIGHT)
        } else if geometry.music_wide {
            (MUSIC_WINDOW_WIDTH, COLLAPSED_WINDOW_HEIGHT)
        } else {
            (COLLAPSED_WINDOW_WIDTH, COLLAPSED_WINDOW_HEIGHT)
        };
        let _ = window.set_size(LogicalSize::new(w, h));
        if was_acrylic {
            geometry.panel_acrylic = crate::panel_backdrop::configure(&window, true).await?;
        }
        if expanded && native_active {
            native.set_visible(true);
        }
        return Err("POSITION_FAILED".into());
    }
    let acrylic = if expanded {
        match crate::panel_backdrop::configure(&window, true).await {
            Ok(active) => active,
            Err(error) => {
                // Keep the collapsed island available if the panel backdrop fails.
                let prior_width = if geometry.music_wide { MUSIC_WINDOW_WIDTH } else { COLLAPSED_WINDOW_WIDTH };
                let _ = window.set_size(LogicalSize::new(prior_width, COLLAPSED_WINDOW_HEIGHT));
                let _ = window.set_position(PhysicalPosition::new(before.x, before.y));
                if native_active {
                    let _ = window.hide();
                    native.set_visible(true);
                } else {
                    let _ = window.show();
                }
                return Err(error);
            }
        }
    } else {
        false
    };
    geometry.panel_acrylic = acrylic;
    geometry.expanded = expanded;
    geometry.origin = Some(position);
    geometry.anchor = Some(anchor);
    drop(geometry);

    if !expanded {
        native.rearm_recovery();
    }

    if expanded {
        window.show().map_err(|_| "WINDOW_SHOW_FAILED")?;
    } else if native_active {
        native.set_position(position.x, position.y);
        native.set_visible(
            window
                .app_handle()
                .state::<crate::desktop::DesktopState>()
                .orb_shown(),
        );
        window.hide().map_err(|_| "WINDOW_HIDE_FAILED")?;
    } else {
        window.show().map_err(|_| "WINDOW_SHOW_FAILED")?;
    }
    Ok(acrylic)
}

pub async fn sync_native_position(app: &AppHandle, x: i32, y: i32, edge: u8) -> Result<(), String> {
    let window = app.get_webview_window("main").ok_or("WINDOW_UNAVAILABLE")?;
    let position = SavedPosition {
        version: 1,
        x,
        y,
        dock_edge: edge,
    };
    let state = app.state::<FloatingState>();
    let mut geometry = state.0.lock().await;
    // A queued dock movement must not reposition the already-expanded panel.
    if geometry.expanded {
        return Ok(());
    }
    window
        .set_position(PhysicalPosition::new(x, y))
        .map_err(|_| "POSITION_FAILED")?;
    geometry.anchor = Some(position);
    geometry.origin = Some(position);
    write_position(&window, position)
}

pub async fn show_floating(app: &AppHandle) -> Result<(), String> {
    let window = app.get_webview_window("main").ok_or("WINDOW_UNAVAILABLE")?;
    let state = app.state::<FloatingState>();
    let geometry = state.0.lock().await;
    let expanded = geometry.expanded;
    let position = geometry.anchor.unwrap_or(current(&window)?);
    drop(geometry);
    let native = app.state::<NativeOrbState>();
    if !expanded {
        native.rearm_recovery();
        let status = native.ensure_started(position.x, position.y);
        if status.active {
            native.set_position(position.x, position.y);
            native.set_visible(true);
            return window.hide().map_err(|_| "WINDOW_HIDE_FAILED".into());
        }
    }
    native.set_visible(false);
    window.show().map_err(|_| "WINDOW_SHOW_FAILED".into())
}

pub fn hide_floating(app: &AppHandle) -> Result<(), String> {
    app.state::<NativeOrbState>().set_visible(false);
    app.get_webview_window("main")
        .ok_or("WINDOW_UNAVAILABLE")?
        .hide()
        .map_err(|_| "WINDOW_HIDE_FAILED".into())
}

pub async fn show_legacy_fallback(app: &AppHandle) -> Result<(), String> {
    app.state::<NativeOrbState>().set_visible(false);
    if !app.state::<crate::desktop::DesktopState>().orb_shown() {
        return hide_floating(app);
    }
    let result = app
        .get_webview_window("main")
        .ok_or("WINDOW_UNAVAILABLE")?
        .show()
        .map_err(|_| "WINDOW_SHOW_FAILED".into());
    if result.is_ok() {
        crate::startup::mark_orb_ready(app);
    }
    result
}

/// Retry only a visible collapsed orb. Hold geometry until host handoff finishes,
/// so an expansion cannot be hidden by a concurrent recovery attempt.
pub async fn recover_native_orb(app: &AppHandle) -> Result<(), String> {
    let native = app.state::<NativeOrbState>();
    if !native.recovery_due() {
        return Ok(());
    }
    let desktop = app.state::<crate::desktop::DesktopState>();
    let state = app.state::<FloatingState>();
    let mut geometry = state.0.lock().await;
    if desktop.is_quitting() || !desktop.orb_shown() || geometry.expanded {
        return Ok(());
    }
    let window = app.get_webview_window("main").ok_or("WINDOW_UNAVAILABLE")?;
    track_drag(&window, &mut geometry)?;
    let position = restore_position(
        geometry.anchor.unwrap_or(current(&window)?),
        &areas(&window)?,
        false,
    );
    let status = native.ensure_started(position.x, position.y);
    if status.active {
        native.configure_dock(native.dock_enabled(), Some(position.dock_edge));
        native.set_visible(desktop.orb_shown() && !desktop.is_quitting());
        window.hide().map_err(|_| "WINDOW_HIDE_FAILED")?;
        let _ = window.emit("native-orb-recovered", ());
    }
    Ok(())
}

async fn snap_collapsed_to_top_center(
    window: &WebviewWindow,
    state: &tauri::State<'_, FloatingState>,
    force: bool,
) -> Result<(), String> {
    let mut geometry = state.0.lock().await;
    track_drag(window, &mut geometry)?;
    let auto_snap = force
        || window
            .app_handle()
            .state::<crate::desktop::DesktopState>()
            .snapshot()
            .map(|s| s.auto_snap_top_center)
            .unwrap_or(true);
    if !auto_snap {
        if let Some(anchor) = geometry.anchor {
            write_position(window, anchor)?;
        }
        return Ok(());
    }
    let work_areas = areas(window)?;
    let primary = window.primary_monitor().ok().flatten().map(|monitor| {
        let area = monitor.work_area();
        WorkArea {
            x: area.position.x,
            y: area.position.y,
            width: area.size.width,
            height: area.size.height,
            scale: monitor.scale_factor(),
        }
    });
    let from = current(window)?;
    let anchor = match primary.or_else(|| work_areas.first().copied()) {
        Some(area) => top_center_island_position(area),
        None => snap_top_center(geometry.anchor.unwrap_or(from), &work_areas),
    };
    if geometry.expanded {
        geometry.anchor = Some(anchor);
        write_position(window, anchor)?;
        return Ok(());
    }
    let target = music_position(anchor, &work_areas);
    if (from.x, from.y) != (target.x, target.y) {
        let dx = f64::from(target.x - from.x);
        let dy = f64::from(target.y - from.y);
        let steps = 10;
        for step in 1..=steps {
            let t = f64::from(step) / f64::from(steps);
            let eased = 1.0 - (1.0 - t).powi(3);
            let x = (f64::from(from.x) + dx * eased).round() as i32;
            let y = (f64::from(from.y) + dy * eased).round() as i32;
            let _ = window.set_position(PhysicalPosition::new(x, y));
            if step < steps {
                tokio::time::sleep(std::time::Duration::from_millis(14)).await;
            }
        }
        window
            .set_position(PhysicalPosition::new(target.x, target.y))
            .map_err(|_| "POSITION_FAILED")?;
    }
    let native = window.app_handle().state::<NativeOrbState>();
    if native.status().active {
        native.set_position(target.x, target.y);
    }
    geometry.anchor = Some(anchor);
    geometry.origin = Some(target);
    write_position(window, anchor)
}

pub async fn snap_now(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let state = app.state::<FloatingState>();
        let _ = snap_collapsed_to_top_center(&window, &state, true).await;
    }
}

#[tauri::command]
pub async fn save_floating_position(
    window: WebviewWindow,
    state: tauri::State<'_, FloatingState>,
) -> Result<(), String> {
    snap_collapsed_to_top_center(&window, &state, false).await
}

#[tauri::command]
pub async fn drag_floating_window(
    window: WebviewWindow,
    state: tauri::State<'_, FloatingState>,
) -> Result<(), String> {
    window
        .start_dragging()
        .map_err(|_| String::from("WINDOW_DRAG_FAILED"))?;
    let _ = snap_collapsed_to_top_center(&window, &state, false).await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn position_serialization_roundtrip_and_invalid_data() {
        let p = SavedPosition {
            version: 1,
            dock_edge: 0,
            x: -123,
            y: 456,
        };
        assert_eq!(
            decode_position(&serde_json::to_string(&p).unwrap()),
            Some(p)
        );
        for raw in [
            "{}",
            "bad",
            r#"{"version":2,"x":1,"y":2}"#,
            r#"{"version":1,"x":1,"y":2,"secret":"x"}"#,
        ] {
            assert_eq!(decode_position(raw), None);
        }
    }
    #[test]
    fn restore_clamps_to_connected_monitor_work_area_at_its_dpi() {
        let areas = [
            WorkArea {
                x: -1920,
                y: 0,
                width: 1920,
                height: 1040,
                scale: 1.5,
            },
            WorkArea {
                x: 0,
                y: 0,
                width: 1920,
                height: 1040,
                scale: 1.0,
            },
        ];
        let p = SavedPosition {
            version: 1,
            dock_edge: 0,
            x: -100,
            y: 1000,
        };
        assert_eq!(
            restore_position(p, &areas, true),
            SavedPosition {
                version: 1,
                dock_edge: 0,
                x: -1140,
                y: 335
            }
        );
        let off = SavedPosition {
            version: 1,
            dock_edge: 0,
            x: 99999,
            y: -500,
        };
        assert_eq!(
            restore_position(off, &areas, false),
            SavedPosition {
                version: 1,
                dock_edge: 0,
                x: 1640,
                y: 0
            }
        );
        let on = SavedPosition {
            version: 1,
            dock_edge: 0,
            x: 200,
            y: 200,
        };
        assert_eq!(restore_position(on, &areas, false), on);
        assert_eq!(restore_position(on, &[], false), on);
    }

    #[test]
    fn island_starts_at_top_center_and_panel_keeps_its_center() {
        let area = WorkArea {
            x: 0,
            y: 0,
            width: 1920,
            height: 1040,
            scale: 1.0,
        };
        let island = default_island_position(area);
        assert_eq!((island.x, island.y), (820, 8));
        let panel = panel_position(island, &[area]);
        assert_eq!((panel.x, panel.y), (580, 8));
        assert_eq!(island.x + 140, panel.x + 380);
    }

    #[test]
    fn music_island_keeps_center_and_stays_inside_the_monitor() {
        let area = WorkArea { x: 0, y: 0, width: 1920, height: 1040, scale: 1.0 };
        let island = default_island_position(area);
        let music = music_position(island, &[area]);
        assert_eq!((music.x, music.y), (740, 8));
        assert_eq!(island.x + 140, music.x + 220);
        let edge = SavedPosition { x: 0, y: 8, ..island };
        assert_eq!(music_position(edge, &[area]).x, 0);
        let right = SavedPosition { x: 1640, y: 8, ..island };
        assert_eq!(music_position(right, &[area]).x, 1480);
        let scaled = WorkArea { x: -1920, y: 0, width: 1920, height: 1040, scale: 1.5 };
        let original = default_island_position(scaled);
        let music = music_position(original, &[scaled]);
        assert_eq!(original.x + 210, music.x + 330);
        assert_eq!(music_position(original, &[]), original);
    }

    #[test]
    fn island_and_panel_are_clamped_on_scaled_monitor() {
        let area = WorkArea {
            x: -1920,
            y: 0,
            width: 1920,
            height: 1040,
            scale: 1.5,
        };
        let island = default_island_position(area);
        assert_eq!((island.x, island.y), (-1170, 12));
        let panel = panel_position(island, &[area]);
        assert_eq!(panel.x, -1530);
        let at_left_edge = SavedPosition {
            x: -1920,
            y: 0,
            ..island
        };
        assert_eq!(panel_position(at_left_edge, &[area]).x, -1920);
    }

    #[test]
    fn dock_position_migrates_old_files_and_preserves_edge() {
        let old = decode_position(r#"{"version":1,"x":20,"y":30}"#).unwrap();
        assert_eq!(old.dock_edge, 0);
        for edge in [1, 2] {
            let saved = SavedPosition {
                dock_edge: edge,
                ..old
            };
            assert_eq!(
                decode_position(&serde_json::to_string(&saved).unwrap()),
                Some(saved)
            );
        }
        assert!(decode_position(r#"{"version":1,"x":20,"y":30,"dock_edge":3}"#).is_none());
    }

    #[test]
    fn dock_restore_uses_ball_center_not_margin_on_neighbor_monitor() {
        let areas = [
            WorkArea {
                x: 0,
                y: 0,
                width: 1920,
                height: 1040,
                scale: 1.0,
            },
            WorkArea {
                x: 1920,
                y: 0,
                width: 2560,
                height: 1400,
                scale: 1.5,
            },
        ];
        let saved = SavedPosition {
            version: 1,
            x: 1914,
            y: 240,
            dock_edge: 1,
        };
        let restored = restore_position(saved, &areas, false);
        assert_eq!(restored.x, 1920);
        assert_eq!(restored.dock_edge, 1);
        assert_eq!(restored.y, 240);
    }
}
