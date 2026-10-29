use codex_flow_lib::codex::protocol::{
    GetAccountRateLimitsResponse, GetAccountResponse, RateLimitResetCreditsSummary,
};
use codex_flow_lib::core::model::{ConnectionState, CoreErrorCode, CoreState, UsageAvailability};

fn rate_limits_fixture() -> GetAccountRateLimitsResponse {
    serde_json::from_str(include_str!("fixtures/five-hour-weekly.json"))
        .expect("fixture must parse")
}

#[test]
fn core_state_starts_disconnected_and_stale() {
    let state = CoreState::default();

    assert_eq!(state.connection, ConnectionState::Disconnected);
    assert!(state.stale);
    assert_eq!(state.error, None);
}

#[test]
fn account_without_account_data_maps_to_not_authenticated() {
    let response: GetAccountResponse =
        serde_json::from_str(r#"{"account":null,"requiresOpenaiAuth":true}"#)
            .expect("account response must parse");
    let mut state = CoreState::default();

    state.apply_account(&response);

    assert_eq!(state.error, Some(CoreErrorCode::NotAuthenticated));
    assert!(state.stale);
}

#[test]
fn account_with_data_and_auth_flag_does_not_infer_not_authenticated() {
    let response: GetAccountResponse = serde_json::from_str(
        r#"{"account":{"type":"chatgpt","planType":"pro"},"requiresOpenaiAuth":true}"#,
    )
    .expect("account response must parse");
    let mut state = CoreState::default();

    state.apply_account(&response);

    assert_eq!(state.error, None);
    assert_eq!(state.account.as_ref().unwrap().authenticated, false);
}

#[test]
fn ordinary_usage_allowed_is_authoritative_over_remaining_window_data() {
    let mut response = rate_limits_fixture();
    response.ordinary_usage_allowed = Some(false);
    let mut state = CoreState::default();

    state.apply_rate_limits(&response);

    assert_eq!(state.usage_availability(), UsageAvailability::Restricted);
    assert_eq!(state.ordinary_usage_allowed, Some(false));
    assert!(state
        .windows
        .iter()
        .any(|window| window.remaining_percent == Some(41)));
}

#[test]
fn reset_credit_count_is_exposed_without_credit_details() {
    let mut response = rate_limits_fixture();
    response.rate_limit_reset_credits = Some(RateLimitResetCreditsSummary {
        available_count: 2,
        credits: None,
    });
    let mut state = CoreState::default();

    state.apply_rate_limits(&response);

    assert_eq!(state.reset_credits_available, Some(2));
}

#[test]
fn stale_transition_preserves_last_known_quota() {
    let response = rate_limits_fixture();
    let mut state = CoreState::default();
    state.apply_rate_limits(&response);

    let known_windows = state.windows.clone();
    state.mark_error(CoreErrorCode::Offline);

    assert_eq!(state.error, Some(CoreErrorCode::Offline));
    assert!(state.stale);
    assert_eq!(state.windows, known_windows);
}

#[test]
fn error_codes_use_stable_wire_values() {
    for (code, expected) in [
        (CoreErrorCode::CodexNotFound, "CODEX_NOT_FOUND"),
        (CoreErrorCode::NotAuthenticated, "NOT_AUTHENTICATED"),
        (CoreErrorCode::UnsupportedCodex, "UNSUPPORTED_CODEX"),
        (CoreErrorCode::Offline, "OFFLINE"),
    ] {
        assert_eq!(
            serde_json::to_string(&code).unwrap(),
            format!("\"{expected}\"")
        );
    }
}
