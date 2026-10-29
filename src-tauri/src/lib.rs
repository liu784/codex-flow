pub mod codex;
pub mod codex_hook;
mod codex_setup;
pub mod core;
pub mod desktop;
#[cfg(feature = "diagnostic-telemetry")]
pub mod diagnostic;
pub mod floating;
mod grok_messages;
mod island_avatar;
mod island_notifications;
pub mod native_orb;
mod panel_backdrop;
mod qrc;
mod qq_music;
pub mod quota;
mod settings;
mod startup;
mod utility_windows;

use tauri::{Emitter, Manager};

#[cfg(all(test, not(debug_assertions)))]
mod release_asset_tests {
    #[test]
    fn release_contains_the_frontend_entry_and_its_script() {
        let context: tauri::Context<tauri::Wry> = tauri::generate_context!();
        assert!(!tauri::is_dev(), "release must not depend on the development server");
        let entry = context.assets().get(&"index.html".into()).expect("missing embedded index.html");
        let html = std::str::from_utf8(&entry).expect("index.html must be UTF-8");
        let script = html.split("src=\"").nth(1).and_then(|value| value.split('"').next())
            .expect("index.html must refer to a frontend script");
        assert!(context.assets().get(&script.trim_start_matches('/').into()).is_some(), "frontend script must be embedded");
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let startup = startup::StartupMetrics::default();
    let core = core::AppCore::new(core::CoreConfig::default());

    tauri::Builder::default()
        // Must remain the first registered plugin: the duplicate process exits
        // before it can initialize a tray, window, renderer, service or timer.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            // Notify the existing process without stealing focus or opening UI.
            let _ = app.emit("second-instance", ());
        }))
        .plugin(
            tauri_plugin_autostart::Builder::new()
                .app_name("Codex Flow")
                .arg("--native-orb")
                .build(),
        )
        .manage(core)
        .manage(startup)
        .manage(desktop::DesktopState::default())
        .manage(island_notifications::NotificationState::default())
        .manage(floating::FloatingState::default())
        .manage(native_orb::NativeOrbState::for_island())
        .on_window_event(|window, event| {
            if !window
                .app_handle()
                .state::<desktop::DesktopState>()
                .is_quitting()
                && matches!(window.label(), "settings" | "tray-popup")
                && matches!(event, tauri::WindowEvent::ScaleFactorChanged { .. })
            {
                if let Some(webview) = window.app_handle().get_webview_window(window.label()) {
                    tauri::async_runtime::spawn(async move {
                        if let Err(error) = panel_backdrop::configure_utility(&webview).await {
                            let _ = webview.emit("utility-error", error);
                        }
                    });
                }
            }
            if window.label() == "tray-popup" && matches!(event, tauri::WindowEvent::Focused(false))
            {
                utility_windows::dismiss_popup(window.app_handle());
            }
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if matches!(window.label(), "settings" | "tray-popup") {
                    api.prevent_close();
                    let _ = window.hide();
                    return;
                }
                if window.label() != "main" {
                    return;
                }
                api.prevent_close();
                if window
                    .app_handle()
                    .state::<desktop::DesktopState>()
                    .snapshot()
                    .is_ok_and(|s| s.minimize_to_tray)
                {
                    let _ = window.emit("floating-collapse", ());
                    return;
                }
                desktop::quit(window.app_handle());
            }
        })
        .invoke_handler(tauri::generate_handler![
            desktop::get_settings,
            desktop::set_settings,
            desktop::reset_settings,
            island_avatar::get_island_avatar,
            island_avatar::set_island_avatar,
            island_notifications::get_island_notification_status,
            island_notifications::enable_island_notifications,
            island_notifications::disable_island_notifications,
            codex_setup::connect_codex_notifications,
            desktop::get_app_info,
            desktop::open_data_directory,
            desktop::get_tray_state,
            desktop::tray_action,
            utility_windows::close_utility,
            utility_windows::drag_settings,
            desktop::open_settings,
            desktop::show_quota_notification_acceptance,
            desktop::get_quota_notification,
            core::service::get_core_state,
            core::service::refresh_core,
            floating::restore_floating_window,
            floating::set_floating_music_wide,
            floating::set_floating_expanded,
            floating::save_floating_position,
            floating::drag_floating_window,
            native_orb::get_native_orb_status,
            native_orb::update_native_orb_visual_state,
        ])
        .setup(|app| {
            desktop::setup(app.handle())?;
            island_notifications::setup(app.handle());
            qq_music::setup(app.handle());
            #[cfg(feature = "gate4-acceptance")]
            if let Some(window) = app.get_webview_window("main") {
                window.show()?;
            }
            let core = app.state::<core::AppCore>().inner().clone();
            let mut state_events = core.subscribe();
            let app_handle = app.handle().clone();

            tauri::async_runtime::spawn(async move {
                loop {
                    match state_events.recv().await {
                        Ok(state) => {
                            let _ = app_handle.emit("core-state", state);
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
                    }
                }
            });

            let core_app = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    let _ = core.start().await;
                    if core_app.state::<desktop::DesktopState>().is_quitting() {
                        return;
                    }
                    while core.has_live_client().await {
                        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                        if core_app.state::<desktop::DesktopState>().is_quitting() {
                            return;
                        }
                    }
                    // A failed initial hydration never blocks the tray or Orb.
                    // Retry in the background without opening a warning window.
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                }
            });

            #[cfg(all(windows, feature = "native-orb-refraction"))]
            if app
                .state::<native_orb::NativeOrbState>()
                .status()
                .feature_enabled
            {
                let native_app = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    let mut interval = tokio::time::interval(std::time::Duration::from_millis(16));
                    loop {
                        interval.tick().await;
                        if native_app.state::<desktop::DesktopState>().is_quitting() {
                            return;
                        }
                        let event = native_app
                            .state::<native_orb::NativeOrbState>()
                            .poll_event();
                        match event {
                            Some(native_orb::NativeOrbEvent::Open) => {
                                let _ = native_app.emit("native-orb-open", ());
                            }
                            Some(native_orb::NativeOrbEvent::Moved { x, y, edge }) => {
                                let _ =
                                    floating::sync_native_position(&native_app, x, y, edge).await;
                            }
                            Some(native_orb::NativeOrbEvent::Failed { reason }) => {
                                native_app
                                    .state::<native_orb::NativeOrbState>()
                                    .fallback(reason.clone());
                                let _ = floating::show_legacy_fallback(&native_app).await;
                                let _ = native_app.emit("native-orb-fallback", reason);
                                // Keep the event pump alive: recovery may install a new host.
                            }
                            None => {}
                        }
                        let _ = floating::recover_native_orb(&native_app).await;
                    }
                });
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Codex Flow");
}
