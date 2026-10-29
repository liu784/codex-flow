use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GetAccountResponse {
    #[serde(default)]
    pub account: Option<Value>,
    #[serde(default)]
    pub requires_openai_auth: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AccountState {
    pub authenticated: bool,
    pub requires_openai_auth: bool,
    pub account_type: Option<String>,
    pub plan_type: Option<String>,
}

pub fn summarize_account(response: &GetAccountResponse) -> AccountState {
    let account = response.account.as_ref().filter(|value| !value.is_null());
    let account_type = account
        .and_then(|value| value.get("type"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    let plan_type = account
        .and_then(|value| value.get("planType"))
        .and_then(Value::as_str)
        .map(str::to_owned);

    AccountState {
        authenticated: account.is_some() && !response.requires_openai_auth,
        requires_openai_auth: response.requires_openai_auth,
        account_type,
        plan_type,
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RateLimitWindow {
    #[serde(default)]
    pub used_percent: Option<i32>,
    #[serde(default)]
    pub window_duration_mins: Option<i64>,
    #[serde(default)]
    pub resets_at: Option<i64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RateLimitSnapshot {
    #[serde(default)]
    pub limit_id: Option<String>,
    #[serde(default)]
    pub limit_name: Option<String>,
    #[serde(default)]
    pub normal_model_slug: Option<String>,
    #[serde(default)]
    pub primary: Option<RateLimitWindow>,
    #[serde(default)]
    pub secondary: Option<RateLimitWindow>,
    #[serde(default)]
    pub credits: Option<serde_json::Value>,
    #[serde(default)]
    pub individual_limit: Option<serde_json::Value>,
    #[serde(default)]
    pub spend_control_reached: Option<bool>,
    #[serde(default)]
    pub plan_type: Option<String>,
    #[serde(default)]
    pub rate_limit_reached_type: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RateLimitResetCredit {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub reset_type: Option<String>,
    #[serde(default)]
    pub granted_at: Option<i64>,
    #[serde(default)]
    pub expires_at: Option<i64>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RateLimitResetCreditsSummary {
    #[serde(default)]
    pub available_count: i64,
    #[serde(default)]
    pub credits: Option<Vec<RateLimitResetCredit>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GetAccountRateLimitsResponse {
    #[serde(default)]
    pub ordinary_usage_allowed: Option<bool>,
    pub rate_limits: RateLimitSnapshot,
    #[serde(default)]
    pub rate_limits_by_limit_id: Option<HashMap<String, RateLimitSnapshot>>,
    #[serde(default)]
    pub rate_limit_reset_credits: Option<RateLimitResetCreditsSummary>,
    #[serde(default)]
    pub account_id: Option<String>,
    #[serde(default)]
    pub rate_limit_upsell: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AccountRateLimitsUpdatedNotification {
    #[serde(default)]
    pub rate_limits: Option<RateLimitSnapshot>,
    #[serde(default)]
    pub ordinary_usage_allowed: Option<bool>,
    #[serde(default)]
    pub rate_limits_by_limit_id: Option<HashMap<String, RateLimitSnapshot>>,
    #[serde(default)]
    pub rate_limit_reset_credits: Option<RateLimitResetCreditsSummary>,
    #[serde(default)]
    pub account_id: Option<String>,
    #[serde(default)]
    pub rate_limit_upsell: Option<serde_json::Value>,
}

impl AccountRateLimitsUpdatedNotification {
    pub fn into_response(self) -> Option<GetAccountRateLimitsResponse> {
        let has_update = self.rate_limits.is_some()
            || self.ordinary_usage_allowed.is_some()
            || self.rate_limits_by_limit_id.is_some()
            || self.rate_limit_reset_credits.is_some()
            || self.account_id.is_some()
            || self.rate_limit_upsell.is_some();
        has_update.then(|| GetAccountRateLimitsResponse {
            ordinary_usage_allowed: self.ordinary_usage_allowed,
            rate_limits: self.rate_limits.unwrap_or_default(),
            rate_limits_by_limit_id: self.rate_limits_by_limit_id,
            rate_limit_reset_credits: self.rate_limit_reset_credits,
            account_id: self.account_id,
            rate_limit_upsell: self.rate_limit_upsell,
        })
    }
}
