use crate::{
    codex::protocol::{summarize_account, GetAccountRateLimitsResponse, GetAccountResponse},
    quota::normalize::{normalize_response, NormalizedQuotaWindow},
};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CoreErrorCode {
    CodexNotFound,
    NotAuthenticated,
    UnsupportedCodex,
    Offline,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Ready,
    Offline,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UsageAvailability {
    Allowed,
    Restricted,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CoreState {
    pub connection: ConnectionState,
    pub error: Option<CoreErrorCode>,
    pub account: Option<crate::codex::protocol::AccountState>,
    pub ordinary_usage_allowed: Option<bool>,
    pub reset_credits_available: Option<i64>,
    pub windows: Vec<NormalizedQuotaWindow>,
    pub stale: bool,
    pub last_success_at: Option<u64>,
}

impl Default for CoreState {
    fn default() -> Self {
        Self {
            connection: ConnectionState::Disconnected,
            error: None,
            account: None,
            ordinary_usage_allowed: None,
            reset_credits_available: None,
            windows: Vec::new(),
            stale: true,
            last_success_at: None,
        }
    }
}

impl CoreState {
    pub fn set_connecting(&mut self) {
        self.connection = ConnectionState::Connecting;
        self.error = None;
        self.stale = true;
    }

    pub fn apply_account(&mut self, response: &GetAccountResponse) {
        let has_account = response
            .account
            .as_ref()
            .map(|account| !account.is_null())
            .unwrap_or(false);
        self.account = Some(summarize_account(response));
        if !has_account {
            self.error = Some(CoreErrorCode::NotAuthenticated);
            self.stale = true;
        } else if self.error == Some(CoreErrorCode::NotAuthenticated) {
            self.error = None;
        }
    }

    pub fn apply_rate_limits(&mut self, response: &GetAccountRateLimitsResponse) {
        let not_authenticated = self.error == Some(CoreErrorCode::NotAuthenticated);
        self.ordinary_usage_allowed = response.ordinary_usage_allowed;
        self.reset_credits_available = response
            .rate_limit_reset_credits
            .as_ref()
            .map(|credits| credits.available_count.max(0));
        self.windows = normalize_response(response);
        self.connection = ConnectionState::Ready;
        self.stale = not_authenticated;
        if !not_authenticated {
            self.error = None;
        }
        self.last_success_at = Some(unix_timestamp());
    }

    pub fn mark_error(&mut self, error: CoreErrorCode) {
        self.error = Some(error);
        self.stale = true;
        if error != CoreErrorCode::NotAuthenticated {
            self.connection = ConnectionState::Offline;
        }
    }

    pub fn usage_availability(&self) -> UsageAvailability {
        match self.ordinary_usage_allowed {
            Some(true) => UsageAvailability::Allowed,
            Some(false) => UsageAvailability::Restricted,
            None => UsageAvailability::Unknown,
        }
    }

    pub fn mark_disconnected(&mut self) {
        self.connection = ConnectionState::Disconnected;
        self.error = None;
        self.stale = true;
    }
}

fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}
