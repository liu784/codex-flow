use crate::codex::protocol::{GetAccountRateLimitsResponse, RateLimitSnapshot, RateLimitWindow};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum QuotaWindowType {
    FiveHour,
    Weekly,
    Custom,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NormalizedQuotaWindow {
    pub limit_id: Option<String>,
    pub plan_type: Option<String>,
    pub source_slot: String,
    pub kind: QuotaWindowType,
    pub duration_minutes: Option<i64>,
    pub used_percent: Option<i32>,
    pub remaining_percent: Option<i32>,
    pub resets_at: Option<i64>,
}

pub fn classify_duration(duration_minutes: Option<i64>) -> QuotaWindowType {
    match duration_minutes {
        Some(300) => QuotaWindowType::FiveHour,
        Some(10_080) => QuotaWindowType::Weekly,
        Some(value) if value > 0 => QuotaWindowType::Custom,
        _ => QuotaWindowType::Unknown,
    }
}

pub fn normalize_window(
    limit_id: Option<String>,
    plan_type: Option<String>,
    source_slot: &str,
    window: &RateLimitWindow,
) -> NormalizedQuotaWindow {
    let used = window.used_percent.map(|v| v.clamp(0, 100));
    NormalizedQuotaWindow {
        limit_id,
        plan_type,
        source_slot: source_slot.to_string(),
        kind: classify_duration(window.window_duration_mins),
        duration_minutes: window.window_duration_mins,
        used_percent: used,
        remaining_percent: used.map(|v| 100 - v),
        resets_at: window.resets_at,
    }
}

pub fn normalize_snapshot(snapshot: &RateLimitSnapshot) -> Vec<NormalizedQuotaWindow> {
    let mut windows = Vec::new();
    if let Some(primary) = &snapshot.primary {
        windows.push(normalize_window(
            snapshot.limit_id.clone(),
            snapshot.plan_type.clone(),
            "primary",
            primary,
        ));
    }
    if let Some(secondary) = &snapshot.secondary {
        windows.push(normalize_window(
            snapshot.limit_id.clone(),
            snapshot.plan_type.clone(),
            "secondary",
            secondary,
        ));
    }
    windows
}

pub fn normalize_response(response: &GetAccountRateLimitsResponse) -> Vec<NormalizedQuotaWindow> {
    if let Some(by_limit_id) = &response.rate_limits_by_limit_id {
        if !by_limit_id.is_empty() {
            let mut keys: Vec<_> = by_limit_id.keys().cloned().collect();
            keys.sort();
            let has_main_codex_bucket = by_limit_id.iter().any(|(key, snapshot)| {
                key.eq_ignore_ascii_case("codex")
                    || snapshot
                        .limit_id
                        .as_deref()
                        .is_some_and(|limit_id| limit_id.eq_ignore_ascii_case("codex"))
            });
            let mut windows: Vec<_> = keys
                .into_iter()
                .flat_map(|key| {
                    let mut snapshot = by_limit_id[&key].clone();
                    if snapshot.limit_id.is_none() {
                        snapshot.limit_id = Some(key);
                    }
                    normalize_snapshot(&snapshot)
                })
                .collect();
            if !has_main_codex_bucket
                && response
                    .rate_limits
                    .limit_id
                    .as_deref()
                    .is_some_and(|limit_id| limit_id.eq_ignore_ascii_case("codex"))
            {
                windows.extend(normalize_snapshot(&response.rate_limits));
            }
            return windows;
        }
    }
    normalize_snapshot(&response.rate_limits)
}

pub fn merge_sparse_snapshot(
    base: &RateLimitSnapshot,
    update: &RateLimitSnapshot,
) -> RateLimitSnapshot {
    RateLimitSnapshot {
        limit_id: update.limit_id.clone().or_else(|| base.limit_id.clone()),
        limit_name: update
            .limit_name
            .clone()
            .or_else(|| base.limit_name.clone()),
        normal_model_slug: update
            .normal_model_slug
            .clone()
            .or_else(|| base.normal_model_slug.clone()),
        primary: merge_sparse_window(base.primary.as_ref(), update.primary.as_ref()),
        secondary: merge_sparse_window(base.secondary.as_ref(), update.secondary.as_ref()),
        credits: update.credits.clone().or_else(|| base.credits.clone()),
        individual_limit: update
            .individual_limit
            .clone()
            .or_else(|| base.individual_limit.clone()),
        spend_control_reached: update.spend_control_reached.or(base.spend_control_reached),
        plan_type: update.plan_type.clone().or_else(|| base.plan_type.clone()),
        rate_limit_reached_type: update
            .rate_limit_reached_type
            .clone()
            .or_else(|| base.rate_limit_reached_type.clone()),
    }
}

pub fn merge_sparse_response(
    base: &GetAccountRateLimitsResponse,
    update: &GetAccountRateLimitsResponse,
) -> GetAccountRateLimitsResponse {
    GetAccountRateLimitsResponse {
        ordinary_usage_allowed: update
            .ordinary_usage_allowed
            .or(base.ordinary_usage_allowed),
        rate_limits: merge_sparse_snapshot(&base.rate_limits, &update.rate_limits),
        rate_limits_by_limit_id: merge_sparse_limit_maps(
            base.rate_limits_by_limit_id.as_ref(),
            update.rate_limits_by_limit_id.as_ref(),
        ),
        rate_limit_reset_credits: update
            .rate_limit_reset_credits
            .clone()
            .or_else(|| base.rate_limit_reset_credits.clone()),
        account_id: update
            .account_id
            .clone()
            .or_else(|| base.account_id.clone()),
        rate_limit_upsell: update
            .rate_limit_upsell
            .clone()
            .or_else(|| base.rate_limit_upsell.clone()),
    }
}

fn merge_sparse_limit_maps(
    base: Option<&HashMap<String, RateLimitSnapshot>>,
    update: Option<&HashMap<String, RateLimitSnapshot>>,
) -> Option<HashMap<String, RateLimitSnapshot>> {
    match (base, update) {
        (None, None) => None,
        (Some(base), None) => Some(base.clone()),
        (None, Some(update)) => Some(update.clone()),
        (Some(base), Some(update)) => {
            let mut merged = base.clone();
            for (key, snapshot) in update {
                let value = merged
                    .get(key)
                    .map(|base| merge_sparse_snapshot(base, snapshot))
                    .unwrap_or_else(|| snapshot.clone());
                merged.insert(key.clone(), value);
            }
            Some(merged)
        }
    }
}

fn merge_sparse_window(
    base: Option<&RateLimitWindow>,
    update: Option<&RateLimitWindow>,
) -> Option<RateLimitWindow> {
    match (base, update) {
        (None, None) => None,
        (Some(base), None) => Some(base.clone()),
        (None, Some(update)) => Some(update.clone()),
        (Some(base), Some(update)) => Some(RateLimitWindow {
            used_percent: update.used_percent.or(base.used_percent),
            window_duration_mins: update.window_duration_mins.or(base.window_duration_mins),
            resets_at: update.resets_at.or(base.resets_at),
        }),
    }
}
