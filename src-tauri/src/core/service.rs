use crate::{
    codex::{
        client::{AppServerClient, AppServerError},
        protocol::{
            AccountRateLimitsUpdatedNotification, GetAccountRateLimitsResponse, GetAccountResponse,
        },
    },
    quota::normalize::merge_sparse_response,
};
use serde_json::Value;
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::{
    env,
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, Weak},
};
use tokio::{
    sync::{broadcast, Mutex, RwLock},
    task::JoinHandle,
    time::{sleep, Duration},
};

use super::model::{CoreErrorCode, CoreState};

const CLIENT_HEALTH_POLL_INTERVAL: Duration = Duration::from_millis(250);
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

#[derive(Debug, Clone, Default)]
pub struct CoreConfig {
    pub codex_executable: Option<PathBuf>,
}

#[derive(Clone)]
pub struct AppCore {
    inner: Arc<CoreInner>,
}

struct CoreInner {
    state: RwLock<CoreState>,
    rate_limits: Mutex<Option<GetAccountRateLimitsResponse>>,
    client: Mutex<Option<AppServerClient>>,
    notification_task: Mutex<Option<JoinHandle<()>>>,
    operation: Mutex<()>,
    state_events: broadcast::Sender<CoreState>,
    config: CoreConfig,
}

impl AppCore {
    pub fn new(config: CoreConfig) -> Self {
        let (state_events, _) = broadcast::channel(32);
        Self {
            inner: Arc::new(CoreInner {
                state: RwLock::new(CoreState::default()),
                rate_limits: Mutex::new(None),
                client: Mutex::new(None),
                notification_task: Mutex::new(None),
                operation: Mutex::new(()),
                state_events,
                config,
            }),
        }
    }

    pub async fn state(&self) -> CoreState {
        self.inner.state.read().await.clone()
    }

    pub fn subscribe(&self) -> broadcast::Receiver<CoreState> {
        self.inner.state_events.subscribe()
    }

    pub async fn start(&self) -> CoreState {
        let _operation = self.inner.operation.lock().await;
        self.refresh_locked().await;
        self.state().await
    }

    pub async fn refresh(&self) -> CoreState {
        let _operation = self.inner.operation.lock().await;
        self.refresh_locked().await;
        self.state().await
    }

    pub async fn shutdown(&self) {
        let _operation = self.inner.operation.lock().await;
        self.stop_notification_task().await;

        let client = self.inner.client.lock().await.take();
        if let Some(client) = client {
            let _ = client.shutdown().await;
        }

        self.update_state(|state| state.mark_disconnected()).await;
    }

    pub async fn has_live_client(&self) -> bool {
        self.inner
            .client
            .lock()
            .await
            .as_ref()
            .map(|client| !client.is_closed())
            .unwrap_or(false)
    }

    pub async fn has_notification_task(&self) -> bool {
        self.inner.notification_task.lock().await.is_some()
    }

    async fn refresh_locked(&self) {
        if !self.ensure_client_locked().await {
            return;
        }

        self.read_snapshot_locked().await;
    }

    async fn ensure_client_locked(&self) -> bool {
        let closed_client = {
            let mut client = self.inner.client.lock().await;
            if let Some(client) = client.as_ref() {
                if !client.is_closed() {
                    return true;
                }
            }
            client.take()
        };

        if let Some(client) = closed_client {
            self.stop_notification_task().await;
            let _ = client.shutdown().await;
        }

        let executable = match locate_codex(self.inner.config.codex_executable.as_deref()) {
            Ok(executable) => executable,
            Err(error) => {
                self.mark_error(error).await;
                return false;
            }
        };

        self.update_state(|state| state.set_connecting()).await;

        match AppServerClient::connect(&executable).await {
            Ok((client, _initialize)) => {
                let notifications = client.subscribe();
                *self.inner.client.lock().await = Some(client);
                self.spawn_notification_task(notifications).await;
                true
            }
            Err(error) => {
                self.record_app_server_error(&error).await;
                false
            }
        }
    }

    async fn read_snapshot_locked(&self) {
        let account = match self.account_read().await {
            Ok(account) => account,
            Err(error) => {
                self.record_app_server_error(&error).await;
                return;
            }
        };

        let account = match serde_json::from_value::<GetAccountResponse>(account) {
            Ok(account) => account,
            Err(_) => {
                self.mark_error(CoreErrorCode::UnsupportedCodex).await;
                return;
            }
        };
        self.update_state(|state| state.apply_account(&account))
            .await;

        let rate_limits = match self.rate_limits_read().await {
            Ok(rate_limits) => rate_limits,
            Err(error) => {
                self.record_app_server_error(&error).await;
                return;
            }
        };

        let rate_limits = match serde_json::from_value::<GetAccountRateLimitsResponse>(rate_limits)
        {
            Ok(rate_limits) => rate_limits,
            Err(_) => {
                self.mark_error(CoreErrorCode::UnsupportedCodex).await;
                return;
            }
        };

        *self.inner.rate_limits.lock().await = Some(rate_limits.clone());
        self.update_state(|state| state.apply_rate_limits(&rate_limits))
            .await;

        // Usage is an optional capability. Its result is intentionally not retained because the
        // Gate 1 core only exposes account/quota state.
        if let Err(error) = self.usage_read().await {
            if error.is_transport_failure() {
                self.record_app_server_error(&error).await;
            }
        }
    }

    async fn account_read(&self) -> Result<Value, AppServerError> {
        let client = self.inner.client.lock().await;
        match client.as_ref() {
            Some(client) => client.account_read().await,
            None => Err(AppServerError::Closed),
        }
    }

    async fn rate_limits_read(&self) -> Result<Value, AppServerError> {
        let client = self.inner.client.lock().await;
        match client.as_ref() {
            Some(client) => client.rate_limits_read().await,
            None => Err(AppServerError::Closed),
        }
    }

    async fn usage_read(&self) -> Result<Value, AppServerError> {
        let client = self.inner.client.lock().await;
        match client.as_ref() {
            Some(client) => client.usage_read().await,
            None => Err(AppServerError::Closed),
        }
    }

    async fn spawn_notification_task(&self, receiver: broadcast::Receiver<Value>) {
        self.stop_notification_task().await;

        let core = Arc::downgrade(&self.inner);
        let task = tokio::spawn(notification_loop(core, receiver));
        *self.inner.notification_task.lock().await = Some(task);
    }

    async fn stop_notification_task(&self) {
        if let Some(task) = self.inner.notification_task.lock().await.take() {
            task.abort();
        }
    }

    async fn handle_notification(&self, message: Value) {
        if message.get("method").and_then(Value::as_str) != Some("account/rateLimits/updated") {
            return;
        }

        let params = match message.get("params") {
            Some(params) => params,
            None => {
                self.mark_error(CoreErrorCode::UnsupportedCodex).await;
                return;
            }
        };

        let update = match parse_rate_limits_notification(params) {
            Ok(update) => update,
            Err(error) => {
                self.mark_error(error).await;
                return;
            }
        };

        let merged = {
            let mut current = self.inner.rate_limits.lock().await;
            let merged = match current.as_ref() {
                Some(base) => merge_sparse_response(base, &update),
                None => update,
            };
            *current = Some(merged.clone());
            merged
        };
        self.update_state(|state| state.apply_rate_limits(&merged))
            .await;
    }

    async fn record_app_server_error(&self, error: &AppServerError) {
        self.mark_error(map_app_server_error(error)).await;
        if error.is_transport_failure() {
            self.disconnect_client().await;
        }
    }

    async fn disconnect_client(&self) {
        self.stop_notification_task().await;
        let client = self.inner.client.lock().await.take();
        if let Some(client) = client {
            let _ = client.shutdown().await;
        }
    }

    async fn mark_error(&self, error: CoreErrorCode) {
        self.update_state(|state| state.mark_error(error)).await;
    }

    async fn update_state<F>(&self, update: F) -> CoreState
    where
        F: FnOnce(&mut CoreState),
    {
        let state = {
            let mut state = self.inner.state.write().await;
            update(&mut state);
            state.clone()
        };
        let _ = self.inner.state_events.send(state.clone());
        state
    }
}

pub fn parse_rate_limits_notification(
    params: &Value,
) -> Result<GetAccountRateLimitsResponse, CoreErrorCode> {
    serde_json::from_value::<AccountRateLimitsUpdatedNotification>(params.clone())
        .map_err(|_| CoreErrorCode::UnsupportedCodex)?
        .into_response()
        .ok_or(CoreErrorCode::UnsupportedCodex)
}

pub fn map_app_server_error(error: &AppServerError) -> CoreErrorCode {
    match error {
        AppServerError::Rpc {
            method_not_found: true,
            ..
        }
        | AppServerError::Protocol
        | AppServerError::Serialize => CoreErrorCode::UnsupportedCodex,
        AppServerError::Rpc { method, .. }
            if matches!(method.as_str(), "account/read" | "account/rateLimits/read") =>
        {
            CoreErrorCode::NotAuthenticated
        }
        AppServerError::Rpc { .. } => CoreErrorCode::UnsupportedCodex,
        AppServerError::Spawn
        | AppServerError::MissingStdin
        | AppServerError::MissingStdout
        | AppServerError::MissingStderr
        | AppServerError::Timeout { .. }
        | AppServerError::ProcessExited { .. }
        | AppServerError::StreamClosed
        | AppServerError::Closed
        | AppServerError::Io => CoreErrorCode::Offline,
    }
}

pub fn locate_codex(explicit: Option<&Path>) -> Result<PathBuf, CoreErrorCode> {
    if let Some(path) = explicit {
        return path
            .exists()
            .then(|| path.to_path_buf())
            .ok_or(CoreErrorCode::CodexNotFound);
    }

    if let Some(path) = env::var_os("CODEX_FLOW_CODEX_BIN") {
        let path = PathBuf::from(path);
        return path
            .exists()
            .then_some(path)
            .ok_or(CoreErrorCode::CodexNotFound);
    }

    if let Some(path) = env::var_os("CODEX_APP_RUNTIME_CLI_PATH").map(PathBuf::from) {
        if path.is_file() {
            return Ok(path);
        }
    }

    if let Some(local_app_data) = env::var_os("LOCALAPPDATA") {
        let local = PathBuf::from(local_app_data);
        let path = local
            .clone()
            .join("Programs")
            .join("OpenAI")
            .join("Codex")
            .join("bin")
            .join("codex.exe");
        if path.exists() {
            return Ok(path);
        }
        #[cfg(windows)]
        if let Some(path) = installed_desktop_codex(&local.join("OpenAI").join("Codex").join("bin")) {
            return Ok(path);
        }
    }

    #[cfg(windows)]
    let locator = "where.exe";
    #[cfg(not(windows))]
    let locator = "which";

    let mut locator_command = Command::new(locator);
    #[cfg(windows)]
    locator_command.creation_flags(CREATE_NO_WINDOW);
    let output = locator_command
        .arg("codex")
        .output()
        .map_err(|_| CoreErrorCode::CodexNotFound)?;
    if !output.status.success() {
        return Err(CoreErrorCode::CodexNotFound);
    }

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.exists())
        .ok_or(CoreErrorCode::CodexNotFound)
}

// Desktop installs keep versioned executables in bin/<version>/codex.exe.
// Prefer the most recently modified available executable when no override exists.
#[cfg(windows)]
fn installed_desktop_codex(root: &Path) -> Option<PathBuf> {
    std::fs::read_dir(root).ok()?.flatten().filter_map(|entry| {
        let executable = entry.path().join("codex.exe");
        let metadata = std::fs::metadata(&executable).ok()?;
        metadata.is_file().then_some((metadata.modified().ok()?, executable))
    }).max().map(|(_, path)| path)
}

async fn notification_loop(core: Weak<CoreInner>, mut receiver: broadcast::Receiver<Value>) {
    loop {
        tokio::select! {
            message = receiver.recv() => match message {
                Ok(message) => {
                    let Some(inner) = core.upgrade() else {
                        return;
                    };
                    AppCore { inner }.handle_notification(message).await;
                }
                Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(broadcast::error::RecvError::Closed) => return,
            },
            _ = sleep(CLIENT_HEALTH_POLL_INTERVAL) => {
                let Some(inner) = core.upgrade() else {
                    return;
                };
                let closed = inner
                    .client
                    .lock()
                    .await
                    .as_ref()
                    .map(|client| client.is_closed())
                    .unwrap_or(true);
                if closed {
                    AppCore { inner }.mark_error(CoreErrorCode::Offline).await;
                    return;
                }
            }
        }
    }
}

#[tauri::command]
pub async fn get_core_state(core: tauri::State<'_, AppCore>) -> Result<CoreState, String> {
    Ok(core.state().await)
}

#[tauri::command]
pub async fn refresh_core(core: tauri::State<'_, AppCore>) -> Result<CoreState, String> {
    Ok(core.refresh().await)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codex::protocol::{RateLimitSnapshot, RateLimitWindow};

    #[cfg(windows)]
    #[test]
    fn discovers_a_versioned_desktop_installation() {
        let root = std::env::temp_dir().join(format!("codex-flow-locate-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let version = root.join("version");
        std::fs::create_dir_all(&version).unwrap();
        assert!(installed_desktop_codex(&root).is_none());
        let exe = version.join("codex.exe");
        std::fs::write(&exe, b"fixture").unwrap();
        assert_eq!(installed_desktop_codex(&root), Some(exe.clone()));
        std::fs::remove_file(exe).unwrap();
        std::fs::remove_dir(version).unwrap();
        std::fs::remove_dir(root).unwrap();
    }

    #[test]
    fn maps_transport_protocol_and_authentication_errors() {
        assert_eq!(
            map_app_server_error(&AppServerError::Timeout {
                method: "account/read".into()
            }),
            CoreErrorCode::Offline
        );
        assert_eq!(
            map_app_server_error(&AppServerError::Protocol),
            CoreErrorCode::UnsupportedCodex
        );
        assert_eq!(
            map_app_server_error(&AppServerError::Rpc {
                method: "account/read".into(),
                code: Some(-32600),
                method_not_found: false,
            }),
            CoreErrorCode::NotAuthenticated
        );
        assert_eq!(
            map_app_server_error(&AppServerError::Rpc {
                method: "account/rateLimits/updated".into(),
                code: Some(-32601),
                method_not_found: true,
            }),
            CoreErrorCode::UnsupportedCodex
        );
    }

    #[test]
    fn malformed_notification_is_an_error_without_panicking() {
        let result = parse_rate_limits_notification(&serde_json::json!({
            "rateLimits": {"primary": {"usedPercent": "invalid"}}
        }));
        assert_eq!(result, Err(CoreErrorCode::UnsupportedCodex));
    }

    #[test]
    fn notification_can_update_only_the_limit_map() {
        let result = parse_rate_limits_notification(&serde_json::json!({
            "rateLimitsByLimitId": {
                "codex": {"primary": {"usedPercent": 20}}
            }
        }))
        .expect("limit-map notification must parse");

        assert!(result.rate_limits.primary.is_none());
        assert!(result.rate_limits_by_limit_id.is_some());
    }

    #[tokio::test]
    async fn notification_merges_into_last_complete_snapshot() {
        let core = AppCore::new(CoreConfig::default());
        let base = GetAccountRateLimitsResponse {
            ordinary_usage_allowed: Some(true),
            rate_limits: RateLimitSnapshot {
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
            },
            ..Default::default()
        };
        *core.inner.rate_limits.lock().await = Some(base.clone());
        core.update_state(|state| state.apply_rate_limits(&base))
            .await;

        core.handle_notification(serde_json::json!({
            "method": "account/rateLimits/updated",
            "params": {
                "rateLimits": {"primary": {"usedPercent": 15}}
            }
        }))
        .await;

        let state = core.state().await;
        assert_eq!(state.windows.len(), 2);
        assert_eq!(state.windows[0].used_percent, Some(15));
        assert_eq!(state.windows[0].duration_minutes, Some(300));
        assert_eq!(state.windows[1].duration_minutes, Some(10080));
        assert_eq!(state.ordinary_usage_allowed, Some(true));
    }

    #[tokio::test]
    async fn missing_codex_is_explicit_and_does_not_create_a_client() {
        let core = AppCore::new(CoreConfig {
            codex_executable: Some(PathBuf::from("Z:\\codex-flow-missing\\codex.exe")),
        });

        let state = core.start().await;

        assert_eq!(state.error, Some(CoreErrorCode::CodexNotFound));
        assert!(state.stale);
        assert!(!core.has_live_client().await);
    }
}
