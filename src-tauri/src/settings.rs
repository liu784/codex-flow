//! The single persisted preference schema. No quota or health calculation lives here.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct Settings {
    pub settings_schema_version: u8,
    pub auto_start: bool,
    pub always_on_top: bool,
    pub minimize_to_tray: bool,
    pub main_panel_enabled: bool,
    pub auto_snap_top_center: bool,
    pub orb_hover_reveal: bool,
    pub orb_edge_dock_enabled: bool,
    pub orb_half_hide: bool,
    pub marquee: bool,
    pub eye_tracking: bool,
    pub panel_opening_animation: bool,
    pub reduce_motion: bool,
    pub quota_alerts_enabled: bool,
    pub warning_enabled: bool,
    pub critical_enabled: bool,
    pub refresh_interval: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            settings_schema_version: 1,
            auto_start: false,
            always_on_top: true,
            minimize_to_tray: true,
            main_panel_enabled: true,
            auto_snap_top_center: true,
            orb_hover_reveal: true,
            orb_edge_dock_enabled: true,
            orb_half_hide: true,
            marquee: true,
            eye_tracking: true,
            panel_opening_animation: true,
            reduce_motion: false,
            quota_alerts_enabled: true,
            warning_enabled: true,
            critical_enabled: true,
            refresh_interval: 0,
        }
    }
}

#[derive(Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SettingsPatch {
    pub auto_start: Option<bool>,
    pub always_on_top: Option<bool>,
    pub minimize_to_tray: Option<bool>,
    pub main_panel_enabled: Option<bool>,
    pub auto_snap_top_center: Option<bool>,
    pub orb_hover_reveal: Option<bool>,
    pub orb_edge_dock_enabled: Option<bool>,
    pub orb_half_hide: Option<bool>,
    pub marquee: Option<bool>,
    pub eye_tracking: Option<bool>,
    pub panel_opening_animation: Option<bool>,
    pub reduce_motion: Option<bool>,
    pub quota_alerts_enabled: Option<bool>,
    pub warning_enabled: Option<bool>,
    pub critical_enabled: Option<bool>,
    pub refresh_interval: Option<u32>,
}

impl Settings {
    pub fn validate(self) -> Result<Self, String> {
        if self.settings_schema_version != 1 {
            return Err("SETTINGS_VERSION_UNSUPPORTED".into());
        }
        if ![0, 60, 300, 600, 1800].contains(&self.refresh_interval) {
            return Err("SETTINGS_INTERVAL_INVALID".into());
        }
        Ok(self)
    }
    pub fn patched(mut self, patch: SettingsPatch) -> Result<Self, String> {
        macro_rules! apply { ($($field:ident),*) => { $(if let Some(value) = patch.$field { self.$field = value; })* }; }
        apply!(
            auto_start,
            always_on_top,
            minimize_to_tray,
            main_panel_enabled,
            auto_snap_top_center,
            orb_hover_reveal,
            orb_edge_dock_enabled,
            orb_half_hide,
            marquee,
            eye_tracking,
            panel_opening_animation,
            reduce_motion,
            quota_alerts_enabled,
            warning_enabled,
            critical_enabled,
            refresh_interval
        );
        self.validate()
    }
    pub fn native_options(self) -> u32 {
        u32::from(self.always_on_top)
            | (u32::from(self.marquee && !self.reduce_motion) << 1)
            | (u32::from(self.orb_hover_reveal) << 2)
            | (u32::from(self.orb_half_hide) << 3)
            | (u32::from(self.reduce_motion) << 4)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_preferences_migrate_without_resetting_switches() {
        let settings: Settings =
            serde_json::from_str(r#"{"quotaAlertsEnabled":false,"orbEdgeDockEnabled":false}"#)
                .unwrap();
        assert!(!settings.quota_alerts_enabled && !settings.orb_edge_dock_enabled);
        assert!(settings.eye_tracking && settings.marquee && settings.orb_half_hide);
        assert_eq!(settings.validate().unwrap().settings_schema_version, 1);
    }
    #[test]
    fn partial_patch_preserves_other_fields_and_validates_interval() {
        let old = Settings::default();
        let next = old
            .patched(SettingsPatch {
                orb_edge_dock_enabled: Some(false),
                ..Default::default()
            })
            .unwrap();
        assert!(!next.orb_edge_dock_enabled);
        assert_eq!(next.eye_tracking, old.eye_tracking);
        assert!(old
            .patched(SettingsPatch {
                refresh_interval: Some(2),
                ..Default::default()
            })
            .is_err());
        for interval in [0, 60, 300, 600, 1800] {
            assert!(old
                .patched(SettingsPatch {
                    refresh_interval: Some(interval),
                    ..Default::default()
                })
                .is_ok());
        }
    }
    #[test]
    fn schema_round_trip_and_invalid_fields() {
        let initial = Settings::default();
        assert_eq!(
            initial,
            serde_json::from_slice(&serde_json::to_vec(&initial).unwrap()).unwrap()
        );
        assert!(serde_json::from_str::<Settings>(r#"{"unknown":true}"#).is_err());
        assert!(Settings {
            settings_schema_version: 99,
            ..initial
        }
        .validate()
        .is_err());
        assert_eq!(initial.native_options(), 15);
        assert_eq!(
            Settings {
                reduce_motion: true,
                ..initial
            }
            .native_options(),
            29
        );
    }
}
