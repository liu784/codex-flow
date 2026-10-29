use serde::{Deserialize, Serialize};
use std::ffi::OsStr;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NativeOrbHealth {
    Neutral,
    Healthy,
    Warning,
    Critical,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NativeOrbVisualState {
    pub percentage: Option<u8>,
    pub health: NativeOrbHealth,
    pub visible: bool,
    pub collapsed: bool,
}

impl NativeOrbVisualState {
    fn validate(self) -> Result<Self, String> {
        if self.percentage.is_some_and(|value| value > 100) {
            return Err("NATIVE_ORB_PERCENTAGE_INVALID".into());
        }
        Ok(self)
    }
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NativeOrbStatus {
    pub feature_compiled: bool,
    pub feature_enabled: bool,
    pub active: bool,
    pub fallback_reason: Option<String>,
    pub capture_fps: Option<f64>,
    pub render_fps: Option<f64>,
    pub dpi: Option<u32>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum NativeOrbEvent {
    Open,
    Moved { x: i32, y: i32, edge: u8 },
    Failed { reason: String },
}

#[derive(Debug)]
struct Lifecycle {
    feature_enabled: bool,
    active: bool,
    fallback_reason: Option<String>,
    visual: NativeOrbVisualState,
    recovery_attempts: usize,
    retry_at: Option<Instant>,
    active_since: Option<Instant>,
    stopped: bool,
}

impl Lifecycle {
    fn new(feature_enabled: bool) -> Self {
        Self {
            feature_enabled,
            active: false,
            fallback_reason: None,
            recovery_attempts: 0,
            retry_at: None,
            active_since: None,
            stopped: false,
            visual: NativeOrbVisualState {
                percentage: None,
                health: NativeOrbHealth::Neutral,
                visible: false,
                collapsed: true,
            },
        }
    }

    fn fallback(&mut self, reason: impl Into<String>) {
        self.failed_at(reason.into(), Instant::now());
    }

    fn failed_at(&mut self, reason: String, now: Instant) {
        if self
            .active_since
            .is_some_and(|at| now.saturating_duration_since(at) >= Duration::from_secs(60))
        {
            self.recovery_attempts = 0;
        }
        self.active = false;
        self.active_since = None;
        self.retry_at = [1, 3, 10]
            .get(self.recovery_attempts)
            .map(|seconds| now + Duration::from_secs(*seconds));
        self.fallback_reason = Some(reason);
    }

    fn can_start(&self, now: Instant) -> bool {
        self.feature_enabled
            && !self.active
            && !self.stopped
            && (self.fallback_reason.is_none() || self.retry_at.is_some_and(|at| now >= at))
    }

    fn begin_start(&mut self) {
        if self.fallback_reason.is_some() {
            self.recovery_attempts += 1;
        }
        self.retry_at = None;
    }

    fn started(&mut self, now: Instant) {
        self.active = true;
        self.fallback_reason = None;
        self.retry_at = None;
        self.active_since = Some(now);
    }

    fn rearm_after_user_action(&mut self) {
        if !self.stopped
            && !self.active
            && self.fallback_reason.is_some()
            && self.retry_at.is_none()
        {
            self.recovery_attempts = 0;
            self.retry_at = Some(Instant::now());
        }
    }

    fn status(&self) -> NativeOrbStatus {
        NativeOrbStatus {
            feature_compiled: cfg!(all(windows, feature = "native-orb-refraction")),
            feature_enabled: self.feature_enabled,
            active: self.active,
            fallback_reason: self.fallback_reason.clone(),
            capture_fps: None,
            render_fps: None,
            dpi: None,
        }
    }
}

pub struct NativeOrbState {
    lifecycle: Mutex<Lifecycle>,
    operation: Mutex<()>,
    dock_enabled: AtomicBool,
    options: AtomicU32,
    #[cfg(all(windows, feature = "native-orb-refraction"))]
    backend: Mutex<Option<backend::Handle>>,
}

impl NativeOrbState {
    // The island uses the main webview; the retired circular host must not
    // take over the collapsed window, even when an old autostart flag remains.
    pub fn for_island() -> Self {
        Self::new(false)
    }

    pub fn from_environment() -> Self {
        Self::new(native_orb_enabled(
            std::env::args_os().any(|arg| arg == "--native-orb")
                || feature_flag_enabled(std::env::var_os("CODEX_FLOW_NATIVE_ORB").as_deref()),
        ))
    }

    fn new(feature_enabled: bool) -> Self {
        Self {
            lifecycle: Mutex::new(Lifecycle::new(feature_enabled)),
            operation: Mutex::new(()),
            dock_enabled: AtomicBool::new(true),
            options: AtomicU32::new(15),
            #[cfg(all(windows, feature = "native-orb-refraction"))]
            backend: Mutex::new(None),
        }
    }

    pub fn status(&self) -> NativeOrbStatus {
        let mut status = self
            .lifecycle
            .lock()
            .map(|state| state.status())
            .unwrap_or_else(|_| lock_failed_status());
        #[cfg(all(windows, feature = "native-orb-refraction"))]
        if status.active {
            if let Ok(backend) = self.backend.lock() {
                if let Some(handle) = backend.as_ref() {
                    let stats = handle.stats();
                    status.capture_fps = Some(stats.capture_fps);
                    status.render_fps = Some(stats.render_fps);
                    status.dpi = Some(stats.dpi);
                }
            }
        }
        status
    }

    pub fn ensure_started(&self, x: i32, y: i32) -> NativeOrbStatus {
        // Startup, fallback destruction and shutdown cannot create overlapping native hosts.
        let Ok(_operation) = self.operation.lock() else {
            return lock_failed_status();
        };
        let visual = match self.lifecycle.lock() {
            Ok(mut state) => {
                if !state.can_start(Instant::now()) {
                    return state.status();
                }
                state.begin_start();
                state.visual
            }
            Err(_) => return lock_failed_status(),
        };

        #[cfg(not(all(windows, feature = "native-orb-refraction")))]
        {
            self.fallback_unlocked("NATIVE_ORB_NOT_COMPILED");
            let _ = (x, y, visual);
            return self.status();
        }

        #[cfg(all(windows, feature = "native-orb-refraction"))]
        {
            match backend::Handle::start(x, y) {
                Ok(handle) => {
                    handle.configure_options(self.options.load(Ordering::Relaxed));
                    handle.configure_dock(self.dock_enabled(), None);
                    handle.update(visual);
                    handle.set_position(x, y);
                    handle.set_visible(visual.visible && visual.collapsed);
                    match self.backend.lock() {
                        Ok(mut backend) => *backend = Some(handle),
                        Err(_) => {
                            self.fallback_unlocked("NATIVE_ORB_BACKEND_LOCK_FAILED");
                            return self.status();
                        }
                    }
                    if let Ok(mut lifecycle) = self.lifecycle.lock() {
                        lifecycle.started(Instant::now());
                    }
                }
                Err(reason) => self.fallback_unlocked(reason),
            }
            self.status()
        }
    }

    pub fn update(&self, visual: NativeOrbVisualState) -> Result<NativeOrbStatus, String> {
        let visual = visual.validate()?;
        let active = {
            let mut lifecycle = self
                .lifecycle
                .lock()
                .map_err(|_| "NATIVE_ORB_STATE_LOCK_FAILED")?;
            lifecycle.visual = visual;
            lifecycle.active
        };
        #[cfg(all(windows, feature = "native-orb-refraction"))]
        if active {
            if let Ok(backend) = self.backend.lock() {
                if let Some(handle) = backend.as_ref() {
                    handle.update(visual);
                }
            }
        }
        #[cfg(not(all(windows, feature = "native-orb-refraction")))]
        let _ = active;
        Ok(self.status())
    }

    pub fn set_visible(&self, visible: bool) {
        #[cfg(all(windows, feature = "native-orb-refraction"))]
        if let Ok(backend) = self.backend.lock() {
            if let Some(handle) = backend.as_ref() {
                handle.set_visible(visible);
            }
        }
        #[cfg(not(all(windows, feature = "native-orb-refraction")))]
        let _ = visible;
    }

    pub fn set_position(&self, x: i32, y: i32) {
        #[cfg(all(windows, feature = "native-orb-refraction"))]
        if let Ok(backend) = self.backend.lock() {
            if let Some(handle) = backend.as_ref() {
                handle.set_position(x, y);
            }
        }
        #[cfg(not(all(windows, feature = "native-orb-refraction")))]
        let _ = (x, y);
    }

    pub fn position(&self) -> Option<(i32, i32)> {
        #[cfg(all(windows, feature = "native-orb-refraction"))]
        if let Ok(backend) = self.backend.lock() {
            return backend.as_ref().and_then(backend::Handle::position);
        }
        None
    }

    pub fn dock_enabled(&self) -> bool {
        self.dock_enabled.load(Ordering::Relaxed)
    }

    pub fn configure_options(&self, options: u32) {
        self.options.store(options, Ordering::Relaxed);
        #[cfg(all(windows, feature = "native-orb-refraction"))]
        if let Ok(backend) = self.backend.lock() {
            if let Some(handle) = backend.as_ref() {
                handle.configure_options(options);
            }
        }
    }

    pub fn configure_dock(&self, enabled: bool, restore_edge: Option<u8>) {
        self.dock_enabled.store(enabled, Ordering::Relaxed);
        #[cfg(all(windows, feature = "native-orb-refraction"))]
        if let Ok(backend) = self.backend.lock() {
            if let Some(handle) = backend.as_ref() {
                handle.configure_dock(enabled, restore_edge);
            }
        }
        #[cfg(not(all(windows, feature = "native-orb-refraction")))]
        let _ = restore_edge;
    }

    pub fn dock_position(&self) -> Option<crate::floating::SavedPosition> {
        #[cfg(all(windows, feature = "native-orb-refraction"))]
        if let Ok(backend) = self.backend.lock() {
            return backend.as_ref().and_then(backend::Handle::dock_position);
        }
        None
    }

    pub fn poll_event(&self) -> Option<NativeOrbEvent> {
        #[cfg(all(windows, feature = "native-orb-refraction"))]
        if let Ok(backend) = self.backend.lock() {
            return backend.as_ref().and_then(backend::Handle::poll_event);
        }
        None
    }

    pub fn fallback(&self, reason: impl Into<String>) {
        let Ok(_operation) = self.operation.lock() else {
            return;
        };
        self.fallback_unlocked(reason);
    }

    fn fallback_unlocked(&self, reason: impl Into<String>) {
        #[cfg(all(windows, feature = "native-orb-refraction"))]
        if let Ok(mut backend) = self.backend.lock() {
            *backend = None;
        }
        if let Ok(mut lifecycle) = self.lifecycle.lock() {
            lifecycle.fallback(reason);
        }
    }

    pub fn shutdown(&self) {
        let Ok(_operation) = self.operation.lock() else {
            return;
        };
        #[cfg(all(windows, feature = "native-orb-refraction"))]
        if let Ok(mut backend) = self.backend.lock() {
            *backend = None;
        }
        if let Ok(mut lifecycle) = self.lifecycle.lock() {
            lifecycle.active = false;
            lifecycle.stopped = true;
            lifecycle.retry_at = None;
        }
    }

    pub fn recovery_due(&self) -> bool {
        self.lifecycle
            .lock()
            .is_ok_and(|state| state.fallback_reason.is_some() && state.can_start(Instant::now()))
    }

    pub fn rearm_recovery(&self) {
        if let Ok(mut state) = self.lifecycle.lock() {
            state.rearm_after_user_action();
        }
    }
}

fn native_orb_enabled(runtime_opt_in: bool) -> bool {
    cfg!(all(windows, feature = "native-orb-refraction")) || runtime_opt_in
}

fn lock_failed_status() -> NativeOrbStatus {
    NativeOrbStatus {
        feature_compiled: cfg!(all(windows, feature = "native-orb-refraction")),
        feature_enabled: false,
        active: false,
        fallback_reason: Some("NATIVE_ORB_STATE_LOCK_FAILED".into()),
        capture_fps: None,
        render_fps: None,
        dpi: None,
    }
}

fn feature_flag_enabled(value: Option<&OsStr>) -> bool {
    value.and_then(OsStr::to_str).is_some_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "on"
        )
    })
}

#[tauri::command]
pub fn get_native_orb_status(state: tauri::State<'_, NativeOrbState>) -> NativeOrbStatus {
    state.status()
}

#[tauri::command]
pub fn update_native_orb_visual_state(
    state: tauri::State<'_, NativeOrbState>,
    desktop: tauri::State<'_, crate::desktop::DesktopState>,
    mut visual: NativeOrbVisualState,
) -> Result<NativeOrbStatus, String> {
    // Quota refreshes must not undo an explicit tray Hide action.
    visual.visible &= desktop.orb_shown() && !desktop.is_quitting();
    state.update(visual)
}

#[cfg(all(windows, feature = "native-orb-refraction"))]
mod backend {
    use super::{NativeOrbEvent, NativeOrbHealth, NativeOrbVisualState};
    use std::ffi::c_void;
    use std::ptr::NonNull;

    const EVENT_OPEN: u32 = 1;
    const EVENT_MOVED: u32 = 2;
    const EVENT_FAILURE: u32 = 3;

    #[repr(C)]
    struct FfiVisualState {
        percentage: i32,
        health: u32,
        visible: u32,
        collapsed: u32,
    }

    #[repr(C)]
    #[derive(Default)]
    struct FfiEvent {
        kind: u32,
        x: i32,
        y: i32,
        code: i32,
    }

    #[repr(C)]
    #[derive(Default)]
    pub struct FfiStats {
        pub capture_fps: f64,
        pub render_fps: f64,
        capture_frames: u64,
        pub render_frames: u64,
        pub dpi: u32,
    }

    #[repr(C)]
    #[derive(Default)]
    struct FfiDockPosition {
        x: i32,
        y: i32,
        edge: u32,
    }

    extern "C" {
        fn cf_native_orb_start(
            x: i32,
            y: i32,
            error_stage: *mut u32,
            error_code: *mut u32,
        ) -> *mut c_void;
        fn cf_native_orb_update(handle: *mut c_void, state: FfiVisualState);
        fn cf_native_orb_set_position(handle: *mut c_void, x: i32, y: i32);
        fn cf_native_orb_set_visible(handle: *mut c_void, visible: u32);
        fn cf_native_orb_configure_dock(handle: *mut c_void, enabled: u32, restore_edge: i32);
        fn cf_native_orb_configure_options(handle: *mut c_void, options: u32);
        fn cf_native_orb_get_dock_position(
            handle: *mut c_void,
            position: *mut FfiDockPosition,
        ) -> u32;
        fn cf_native_orb_get_position(handle: *mut c_void, x: *mut i32, y: *mut i32) -> u32;
        fn cf_native_orb_poll_event(handle: *mut c_void, event: *mut FfiEvent) -> u32;
        fn cf_native_orb_get_stats(handle: *mut c_void, stats: *mut FfiStats);
        fn cf_native_orb_shutdown(handle: *mut c_void);
    }

    pub struct Handle(NonNull<c_void>);

    unsafe impl Send for Handle {}

    impl Handle {
        pub fn start(x: i32, y: i32) -> Result<Self, String> {
            let mut error_stage = 0;
            let mut error_code = 0;
            let handle = unsafe { cf_native_orb_start(x, y, &mut error_stage, &mut error_code) };
            NonNull::new(handle).map(Self).ok_or_else(|| {
                let stage = match error_stage {
                    1 => "WINDOW",
                    2 => "CAPTURE_EXCLUSION",
                    3 => "DXGI_CAPTURE",
                    4 => "D3D11",
                    5 => "SHADER",
                    6 => "DIRECTCOMPOSITION",
                    _ => "UNKNOWN",
                };
                format!("NATIVE_ORB_{stage}_FAILED_0x{error_code:08X}")
            })
        }

        pub fn update(&self, visual: NativeOrbVisualState) {
            let health = match visual.health {
                NativeOrbHealth::Neutral => 0,
                NativeOrbHealth::Healthy => 1,
                NativeOrbHealth::Warning => 2,
                NativeOrbHealth::Critical => 3,
            };
            unsafe {
                cf_native_orb_update(
                    self.0.as_ptr(),
                    FfiVisualState {
                        percentage: visual.percentage.map(i32::from).unwrap_or(-1),
                        health,
                        visible: u32::from(visual.visible),
                        collapsed: u32::from(visual.collapsed),
                    },
                )
            }
        }

        pub fn set_position(&self, x: i32, y: i32) {
            unsafe { cf_native_orb_set_position(self.0.as_ptr(), x, y) }
        }

        pub fn set_visible(&self, visible: bool) {
            unsafe { cf_native_orb_set_visible(self.0.as_ptr(), u32::from(visible)) }
        }

        pub fn position(&self) -> Option<(i32, i32)> {
            let (mut x, mut y) = (0, 0);
            let available = unsafe { cf_native_orb_get_position(self.0.as_ptr(), &mut x, &mut y) };
            (available != 0).then_some((x, y))
        }

        pub fn configure_dock(&self, enabled: bool, restore_edge: Option<u8>) {
            unsafe {
                cf_native_orb_configure_dock(
                    self.0.as_ptr(),
                    u32::from(enabled),
                    restore_edge.map(i32::from).unwrap_or(-1),
                )
            }
        }

        pub fn configure_options(&self, options: u32) {
            unsafe { cf_native_orb_configure_options(self.0.as_ptr(), options) }
        }

        pub fn dock_position(&self) -> Option<crate::floating::SavedPosition> {
            let mut p = FfiDockPosition::default();
            let ok = unsafe { cf_native_orb_get_dock_position(self.0.as_ptr(), &mut p) };
            (ok != 0).then_some(crate::floating::SavedPosition {
                version: 1,
                x: p.x,
                y: p.y,
                dock_edge: p.edge.min(2) as u8,
            })
        }

        pub fn poll_event(&self) -> Option<NativeOrbEvent> {
            let mut event = FfiEvent::default();
            let available = unsafe { cf_native_orb_poll_event(self.0.as_ptr(), &mut event) };
            if available == 0 {
                return None;
            }
            match event.kind {
                EVENT_OPEN => Some(NativeOrbEvent::Open),
                EVENT_MOVED => Some(NativeOrbEvent::Moved {
                    x: event.x,
                    y: event.y,
                    edge: event.code.clamp(0, 2) as u8,
                }),
                EVENT_FAILURE => Some(NativeOrbEvent::Failed {
                    reason: format!("NATIVE_ORB_RUNTIME_FAILED_0x{:08X}", event.code as u32),
                }),
                _ => None,
            }
        }

        pub fn stats(&self) -> FfiStats {
            let mut stats = FfiStats::default();
            unsafe { cf_native_orb_get_stats(self.0.as_ptr(), &mut stats) }
            stats
        }
    }

    impl Drop for Handle {
        fn drop(&mut self) {
            unsafe { cf_native_orb_shutdown(self.0.as_ptr()) }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovery_is_delayed_bounded_and_does_not_reset_after_a_brief_success() {
        let mut state = Lifecycle::new(true);
        let start = std::time::Instant::now();
        state.fallback("DXGI_ACCESS_LOST");
        assert!(!state.can_start(start));
        let mut at = start;
        for seconds in [2, 4, 11] {
            at += std::time::Duration::from_secs(seconds);
            assert!(state.can_start(at));
            state.begin_start();
            state.started(at);
            state.failed_at(
                "DXGI_ACCESS_LOST".into(),
                at + std::time::Duration::from_millis(10),
            );
        }
        assert!(!state.can_start(at + std::time::Duration::from_secs(3600)));
        state.rearm_after_user_action();
        assert!(state.can_start(at + std::time::Duration::from_secs(3600)));
    }

    #[test]
    fn recovery_budget_returns_only_after_stable_service_and_shutdown_stays_stopped() {
        let mut state = Lifecycle::new(true);
        let start = std::time::Instant::now();
        state.failed_at("TEMPORARY".into(), start);
        state.begin_start();
        state.started(start);
        state.failed_at(
            "TEMPORARY".into(),
            start + std::time::Duration::from_secs(61),
        );
        assert_eq!(state.recovery_attempts, 0);
        assert!(state.can_start(start + std::time::Duration::from_secs(63)));
        state.stopped = true;
        state.rearm_after_user_action();
        assert!(!state.can_start(start + std::time::Duration::from_secs(3600)));
        assert!(!Lifecycle::new(false).can_start(start));
    }

    #[test]
    fn feature_flag_is_opt_in_and_strict() {
        assert!(!feature_flag_enabled(None));
        assert!(!feature_flag_enabled(Some(OsStr::new("0"))));
        assert!(!feature_flag_enabled(Some(OsStr::new("yes"))));
        for value in ["1", "true", "TRUE", " on "] {
            assert!(feature_flag_enabled(Some(OsStr::new(value))));
        }
    }

    #[test]
    fn visual_state_serializes_only_final_presentation_values() {
        let visual = NativeOrbVisualState {
            percentage: Some(29),
            health: NativeOrbHealth::Warning,
            visible: true,
            collapsed: true,
        };
        assert_eq!(
            serde_json::to_value(visual).unwrap(),
            serde_json::json!({
                "percentage": 29,
                "health": "warning",
                "visible": true,
                "collapsed": true
            })
        );
        assert!(
            serde_json::from_value::<NativeOrbVisualState>(serde_json::json!({
                "percentage": 101,
                "health": "healthy",
                "visible": true,
                "collapsed": true
            }))
            .unwrap()
            .validate()
            .is_err()
        );
    }

    #[test]
    fn fallback_is_explicit_and_keeps_feature_status() {
        let mut lifecycle = Lifecycle::new(true);
        lifecycle.active = true;
        lifecycle.fallback("DXGI_DUPLICATION_FAILED");
        assert_eq!(
            lifecycle.status(),
            NativeOrbStatus {
                feature_compiled: cfg!(all(windows, feature = "native-orb-refraction")),
                feature_enabled: true,
                active: false,
                fallback_reason: Some("DXGI_DUPLICATION_FAILED".into()),
                capture_fps: None,
                render_fps: None,
                dpi: None,
            }
        );
    }

    #[test]
    fn production_feature_is_enabled_without_a_shortcut_argument() {
        assert_eq!(
            native_orb_enabled(false),
            cfg!(all(windows, feature = "native-orb-refraction"))
        );
        assert!(native_orb_enabled(true));
    }

    #[test]
    fn explicitly_disabled_state_does_not_start() {
        let state = NativeOrbState::new(false);
        let status = state.ensure_started(10, 20);
        assert!(!status.feature_enabled);
        assert!(!status.active);
        assert_eq!(status.fallback_reason, None);
    }

    #[cfg(all(windows, feature = "native-orb-refraction"))]
    #[test]
    #[ignore = "requires an interactive Windows desktop"]
    fn native_dock_moves_half_ball_and_restores_without_stopping_rendering() {
        let state = NativeOrbState::new(true);
        let visual = NativeOrbVisualState {
            percentage: Some(61),
            health: NativeOrbHealth::Healthy,
            visible: true,
            collapsed: true,
        };
        state.update(visual).unwrap();
        let started = state.ensure_started(180, 240);
        assert!(started.active, "{:?}", started.fallback_reason);
        for edge in [1, 2] {
            state.configure_dock(true, Some(edge));
            std::thread::sleep(std::time::Duration::from_millis(250));
            let full = state.dock_position().unwrap();
            assert_eq!(full.dock_edge, edge);
            std::thread::sleep(std::time::Duration::from_millis(950));
            let (x, y) = state.position().unwrap();
            let scale = state.status().dpi.unwrap() as f64 / 96.0;
            let delta = (x - full.x) as f64;
            assert!(
                (delta
                    - if edge == 1 {
                        -48.0 * scale
                    } else {
                        48.0 * scale
                    })
                .abs()
                    <= 1.0,
                "edge={edge}, full={}, current={x}; keep pointer away from the test ball",
                full.x
            );
            assert_eq!(y, full.y);
            let frames_before = state
                .backend
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .stats()
                .render_frames;
            state
                .update(NativeOrbVisualState {
                    percentage: Some(60),
                    health: NativeOrbHealth::Warning,
                    ..visual
                })
                .unwrap();
            std::thread::sleep(std::time::Duration::from_millis(100));
            assert_eq!(state.dock_position().unwrap().dock_edge, edge);
            let frames_after = state
                .backend
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .stats()
                .render_frames;
            assert!(
                frames_after > frames_before,
                "half-hidden renderer stopped advancing"
            );
            assert!(state.status().render_fps.unwrap_or_default() > 0.0);
            state.set_visible(false);
            std::thread::sleep(std::time::Duration::from_millis(100));
            state.set_visible(true);
            std::thread::sleep(std::time::Duration::from_millis(100));
            assert!((state.position().unwrap().0 - full.x).abs() <= 1);
            state.configure_dock(false, None);
            std::thread::sleep(std::time::Duration::from_millis(100));
            assert_eq!(state.dock_position().unwrap().dock_edge, 0);
            assert!((state.position().unwrap().0 - full.x).abs() <= 1);
            println!("native edge={edge}: half offset={delta}, restore+disable=PASS");
        }
        state.shutdown();
    }

    #[cfg(all(windows, feature = "native-orb-refraction"))]
    #[test]
    #[ignore = "requires an interactive Windows desktop"]
    fn native_renderer_smoke_uses_the_production_backend() {
        let state = NativeOrbState::new(true);
        state
            .update(NativeOrbVisualState {
                percentage: Some(29),
                health: NativeOrbHealth::Warning,
                visible: true,
                collapsed: true,
            })
            .unwrap();
        let started = state.ensure_started(100, 100);
        assert!(started.active, "{:?}", started.fallback_reason);
        std::thread::sleep(std::time::Duration::from_secs(2));
        let running = state.status();
        println!(
            "NATIVE_ORB_STATS capture_fps={:.2} render_fps={:.2} dpi={}",
            running.capture_fps.unwrap_or_default(),
            running.render_fps.unwrap_or_default(),
            running.dpi.unwrap_or_default()
        );
        assert!(running.capture_fps.is_some_and(|fps| fps > 0.0));
        assert!(running.render_fps.is_some_and(|fps| fps > 0.0));
        assert!(running.dpi.is_some_and(|dpi| dpi >= 96));
        state.shutdown();
    }

    #[cfg(all(windows, feature = "native-orb-refraction"))]
    #[test]
    #[ignore = "requires an interactive Windows desktop; creates only an isolated test orb"]
    fn native_recovery_recreates_real_backend_and_preserves_dock_and_preferences() {
        let state = NativeOrbState::new(true);
        let visual = NativeOrbVisualState {
            percentage: Some(50),
            health: NativeOrbHealth::Healthy,
            visible: true,
            collapsed: true,
        };
        state.configure_options(14); // non-topmost, marquee/hover/half-hide enabled
        state.update(visual).unwrap();
        assert!(state.ensure_started(140, 200).active);
        state.fallback("INJECTED_NATIVE_RUNTIME_FAILURE");
        assert!(!state.status().active);
        assert!(!state.ensure_started(140, 200).active); // no immediate retry
        std::thread::sleep(Duration::from_millis(1100));
        assert!(state.recovery_due());
        assert!(state.ensure_started(140, 200).active);
        assert_eq!(state.options.load(Ordering::Relaxed), 14);
        assert_eq!(state.lifecycle.lock().unwrap().visual, visual);
        state.configure_dock(true, Some(1));
        std::thread::sleep(Duration::from_millis(1150));
        assert_eq!(state.dock_position().unwrap().dock_edge, 1);
        assert!(state.status().render_fps.is_some_and(|fps| fps > 0.));
        state.configure_dock(false, None);
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(state.dock_position().unwrap().dock_edge, 0);
        state.shutdown();
        assert!(!state.ensure_started(140, 200).active);
        println!("RECOVERY_REAL_BACKEND=PASS OPTIONS=14 DOCK_RESTORED=YES FRAMES_ADVANCE=YES SHUTDOWN_STAYS_STOPPED=YES");
    }
}
