use codex_flow_lib::codex::protocol::{
    GetAccountRateLimitsResponse, RateLimitSnapshot, RateLimitWindow,
};
use codex_flow_lib::quota::normalize::{
    classify_duration, merge_sparse_response, merge_sparse_snapshot, normalize_response,
    QuotaWindowType,
};

fn fixture(name: &str) -> GetAccountRateLimitsResponse {
    let raw = match name {
        "five-hour-weekly" => include_str!("fixtures/five-hour-weekly.json"),
        "weekly-only" => include_str!("fixtures/weekly-only.json"),
        "secondary-null" => include_str!("fixtures/secondary-null.json"),
        "custom-window" => include_str!("fixtures/custom-window.json"),
        "malformed-partial" => include_str!("fixtures/malformed-partial.json"),
        _ => panic!("unknown fixture"),
    };
    serde_json::from_str(raw).expect("fixture must parse")
}

#[test]
fn classifies_five_hour_and_weekly_by_duration_not_slot() {
    let windows = normalize_response(&fixture("five-hour-weekly"));
    assert_eq!(windows.len(), 2);
    let five_hour = windows
        .iter()
        .find(|window| window.kind == QuotaWindowType::FiveHour)
        .expect("five-hour window");
    assert_eq!(five_hour.source_slot, "secondary");
    assert_eq!(five_hour.plan_type.as_deref(), Some("pro"));
    assert_eq!(five_hour.remaining_percent, Some(69));
    let weekly = windows
        .iter()
        .find(|window| window.kind == QuotaWindowType::Weekly)
        .expect("weekly window");
    assert_eq!(weekly.source_slot, "primary");
    assert_eq!(weekly.remaining_percent, Some(41));
}

#[test]
fn weekly_only_does_not_fabricate_five_hour_window() {
    let windows = normalize_response(&fixture("weekly-only"));
    assert_eq!(windows.len(), 1);
    assert_eq!(windows[0].kind, QuotaWindowType::Weekly);
    assert!(!windows
        .iter()
        .any(|window| window.kind == QuotaWindowType::FiveHour));
}

#[test]
fn confirmed_weekly_used_percent_derives_remaining_percent() {
    let response: GetAccountRateLimitsResponse = serde_json::from_str(
        r#"{
            "rateLimits": {
                "limitId": "codex",
                "planType": "pro",
                "primary": {
                    "usedPercent": 71,
                    "windowDurationMins": 10080,
                    "resetsAt": 1800500000
                }
            }
        }"#,
    )
    .expect("weekly fixture must parse");

    let windows = normalize_response(&response);
    assert_eq!(windows.len(), 1);
    assert_eq!(windows[0].kind, QuotaWindowType::Weekly);
    assert_eq!(windows[0].used_percent, Some(71));
    assert_eq!(windows[0].remaining_percent, Some(29));
}

#[test]
fn explicit_main_rate_limits_fallback_survives_a_model_specific_limit_map() {
    let response: GetAccountRateLimitsResponse = serde_json::from_str(
        r#"{
            "rateLimits": {
                "limitId": "codex",
                "primary": {"usedPercent": 80, "windowDurationMins": 10080, "resetsAt": 1900000000}
            },
            "rateLimitsByLimitId": {
                "codex_bengalfox": {
                    "limitId": "codex_bengalfox",
                    "primary": {"usedPercent": 0, "windowDurationMins": 300, "resetsAt": 1800000000},
                    "secondary": {"usedPercent": 0, "windowDurationMins": 10080, "resetsAt": 1800100000}
                }
            }
        }"#,
    )
    .expect("mixed bucket fixture must parse");

    let windows = normalize_response(&response);
    assert!(windows.iter().any(|window| {
        window.limit_id.as_deref() == Some("codex")
            && window.kind == QuotaWindowType::Weekly
            && window.used_percent == Some(80)
            && window.remaining_percent == Some(20)
            && window.resets_at == Some(1_900_000_000)
    }));
    assert!(windows
        .iter()
        .any(|window| window.limit_id.as_deref() == Some("codex_bengalfox")));
}

#[test]
fn secondary_null_is_safe() {
    let windows = normalize_response(&fixture("secondary-null"));
    assert_eq!(windows.len(), 1);
    assert_eq!(windows[0].kind, QuotaWindowType::FiveHour);
}

#[test]
fn custom_duration_is_preserved() {
    let windows = normalize_response(&fixture("custom-window"));
    assert_eq!(windows.len(), 1);
    assert_eq!(windows[0].kind, QuotaWindowType::Custom);
    assert_eq!(windows[0].duration_minutes, Some(1440));
}

#[test]
fn missing_and_non_positive_durations_are_unknown() {
    assert_eq!(classify_duration(None), QuotaWindowType::Unknown);
    assert_eq!(classify_duration(Some(0)), QuotaWindowType::Unknown);
    assert_eq!(classify_duration(Some(-1)), QuotaWindowType::Unknown);
}

#[test]
fn sparse_update_keeps_unmentioned_fields() {
    let base = RateLimitSnapshot {
        limit_id: Some("codex".into()),
        plan_type: Some("pro".into()),
        primary: Some(RateLimitWindow {
            used_percent: Some(10),
            window_duration_mins: Some(300),
            resets_at: Some(1000),
        }),
        secondary: Some(RateLimitWindow {
            used_percent: Some(20),
            window_duration_mins: Some(10080),
            resets_at: Some(2000),
        }),
        ..Default::default()
    };
    let update = RateLimitSnapshot {
        primary: Some(RateLimitWindow {
            used_percent: Some(15),
            ..Default::default()
        }),
        ..Default::default()
    };

    let merged = merge_sparse_snapshot(&base, &update);
    let primary = merged.primary.expect("merged primary");
    assert_eq!(primary.used_percent, Some(15));
    assert_eq!(primary.window_duration_mins, Some(300));
    assert_eq!(primary.resets_at, Some(1000));
    let secondary = merged.secondary.expect("preserved secondary");
    assert_eq!(secondary.used_percent, Some(20));
    assert_eq!(merged.plan_type.as_deref(), Some("pro"));
}

#[test]
fn sparse_response_merge_preserves_unmentioned_top_level_and_limit_fields() {
    let base: GetAccountRateLimitsResponse = serde_json::from_str(
        r#"{
            "ordinaryUsageAllowed": true,
            "rateLimits": {
                "limitId": "codex",
                "planType": "pro",
                "primary": {"usedPercent": 10, "windowDurationMins": 300, "resetsAt": 1000},
                "secondary": {"usedPercent": 20, "windowDurationMins": 10080, "resetsAt": 2000}
            },
            "rateLimitsByLimitId": {
                "codex": {"limitId": "codex", "primary": {"usedPercent": 10, "windowDurationMins": 300, "resetsAt": 1000}},
                "other": {"limitId": "other", "primary": {"usedPercent": 30, "windowDurationMins": 1440, "resetsAt": 3000}}
            }
        }"#,
    )
    .expect("base response must parse");
    let update: GetAccountRateLimitsResponse = serde_json::from_str(
        r#"{
            "rateLimits": {"primary": {"usedPercent": 15}},
            "rateLimitsByLimitId": {
                "codex": {"primary": {"usedPercent": 16}}
            }
        }"#,
    )
    .expect("sparse response must parse");

    let merged = merge_sparse_response(&base, &update);

    assert_eq!(merged.ordinary_usage_allowed, Some(true));
    assert_eq!(merged.rate_limits.plan_type.as_deref(), Some("pro"));
    assert_eq!(
        merged.rate_limits.primary.as_ref().unwrap().used_percent,
        Some(15)
    );
    assert_eq!(
        merged
            .rate_limits
            .primary
            .as_ref()
            .unwrap()
            .window_duration_mins,
        Some(300)
    );
    assert!(merged.rate_limits.secondary.is_some());
    let by_limit_id = merged.rate_limits_by_limit_id.unwrap();
    assert!(by_limit_id.contains_key("other"));
    assert_eq!(
        by_limit_id["codex"]
            .primary
            .as_ref()
            .unwrap()
            .window_duration_mins,
        Some(300)
    );
    assert_eq!(
        by_limit_id["codex"].primary.as_ref().unwrap().used_percent,
        Some(16)
    );
}

#[test]
fn partial_and_out_of_range_input_does_not_crash_and_clamps_percent() {
    let windows = normalize_response(&fixture("malformed-partial"));
    assert_eq!(windows.len(), 2);
    assert_eq!(windows[0].kind, QuotaWindowType::Unknown);
    assert_eq!(windows[0].remaining_percent, None);
    assert_eq!(windows[1].kind, QuotaWindowType::Unknown);
    assert_eq!(windows[1].used_percent, Some(100));
    assert_eq!(windows[1].remaining_percent, Some(0));
}

#[test]
fn malformed_json_is_rejected_without_panicking() {
    let result = serde_json::from_str::<GetAccountRateLimitsResponse>(
        r#"{"rateLimits":{"primary":{"usedPercent":"invalid"}}}"#,
    );
    assert!(result.is_err());
}

#[test]
fn every_plan_preserves_duration_classification_in_core_state() {
    use codex_flow_lib::core::model::CoreState;
    for plan in [
        Some("prolite"),
        Some("pro"),
        Some("plus"),
        Some("future"),
        None,
    ] {
        for (duration, expected) in [
            (Some(300), QuotaWindowType::FiveHour),
            (Some(10080), QuotaWindowType::Weekly),
            (Some(60), QuotaWindowType::Custom),
            (None, QuotaWindowType::Unknown),
            (Some(0), QuotaWindowType::Unknown),
            (Some(-1), QuotaWindowType::Unknown),
        ] {
            let response = GetAccountRateLimitsResponse {
                rate_limits: RateLimitSnapshot {
                    plan_type: plan.map(str::to_owned),
                    primary: Some(RateLimitWindow {
                        window_duration_mins: duration,
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut state = CoreState::default();
            state.apply_rate_limits(&response);
            assert_eq!(state.windows.len(), 1);
            assert_eq!(state.windows[0].kind, expected);
            assert_eq!(state.windows[0].duration_minutes, duration);
            assert_eq!(state.windows[0].plan_type.as_deref(), plan);
        }
    }
}

#[test]
fn plus_pro_and_prolite_keep_actual_dual_or_weekly_only_windows() {
    use codex_flow_lib::core::model::CoreState;
    for plan in ["plus", "pro", "prolite"] {
        for fixture_name in ["five-hour-weekly", "weekly-only"] {
            let mut response = fixture(fixture_name);
            response.rate_limits.plan_type = Some(plan.into());
            if let Some(groups) = &mut response.rate_limits_by_limit_id {
                for snapshot in groups.values_mut() {
                    snapshot.plan_type = Some(plan.into());
                }
            }
            let mut state = CoreState::default();
            state.apply_rate_limits(&response);
            let dual = fixture_name == "five-hour-weekly";
            assert_eq!(state.windows.len(), if dual { 2 } else { 1 });
            assert_eq!(
                state
                    .windows
                    .iter()
                    .any(|w| w.kind == QuotaWindowType::FiveHour),
                dual
            );
            assert!(state
                .windows
                .iter()
                .any(|w| w.kind == QuotaWindowType::Weekly));
            assert!(state
                .windows
                .iter()
                .all(|w| w.plan_type.as_deref() == Some(plan)));
        }
    }
}
