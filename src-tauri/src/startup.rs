use serde::Serialize;
use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::Instant,
};
use tauri::Manager;

const UNRECORDED: u64 = u64::MAX;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupSnapshot {
    pub time_to_tray_ms: Option<u64>,
    pub time_to_orb_ms: Option<u64>,
}

pub struct StartupMetrics {
    started: Instant,
    tray_ms: AtomicU64,
    orb_ms: AtomicU64,
}

impl Default for StartupMetrics {
    fn default() -> Self {
        Self {
            started: Instant::now(),
            tray_ms: AtomicU64::new(UNRECORDED),
            orb_ms: AtomicU64::new(UNRECORDED),
        }
    }
}

impl StartupMetrics {
    fn elapsed_ms(&self) -> u64 {
        self.started
            .elapsed()
            .as_millis()
            .min(u128::from(u64::MAX - 1)) as u64
    }

    pub fn mark_tray_ready(&self) {
        let _ = self.tray_ms.compare_exchange(
            UNRECORDED,
            self.elapsed_ms(),
            Ordering::SeqCst,
            Ordering::SeqCst,
        );
    }

    fn mark_orb_ready(&self) -> bool {
        self.orb_ms
            .compare_exchange(
                UNRECORDED,
                self.elapsed_ms(),
                Ordering::SeqCst,
                Ordering::SeqCst,
            )
            .is_ok()
    }

    pub fn snapshot(&self) -> StartupSnapshot {
        fn value(raw: u64) -> Option<u64> {
            (raw != UNRECORDED).then_some(raw)
        }
        StartupSnapshot {
            time_to_tray_ms: value(self.tray_ms.load(Ordering::SeqCst)),
            time_to_orb_ms: value(self.orb_ms.load(Ordering::SeqCst)),
        }
    }
}

pub fn mark_orb_ready(app: &tauri::AppHandle) {
    let metrics = app.state::<StartupMetrics>();
    if !metrics.mark_orb_ready() {
        return;
    }
    let Ok(directory) = app.path().app_local_data_dir() else {
        return;
    };
    if std::fs::create_dir_all(&directory).is_err() {
        return;
    }
    let Ok(data) = serde_json::to_vec(&metrics.snapshot()) else {
        return;
    };
    // Diagnostics are noncritical: a write failure must never hold startup.
    let _ = std::fs::write(directory.join("startup-metrics.json"), data);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn milestones_are_monotonic_and_recorded_only_once() {
        let metrics = StartupMetrics::default();
        metrics.mark_tray_ready();
        assert!(metrics.mark_orb_ready());
        let first = metrics.snapshot();
        assert!(first.time_to_tray_ms.is_some());
        assert!(first.time_to_orb_ms >= first.time_to_tray_ms);
        assert!(!metrics.mark_orb_ready());
        assert_eq!(metrics.snapshot(), first);
    }
}
