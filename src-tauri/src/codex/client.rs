use serde_json::{json, Value};
use std::{
    collections::HashMap,
    path::Path,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
};
use thiserror::Error;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, Command},
    sync::{broadcast, oneshot, Mutex},
    task::JoinHandle,
    time::{sleep, timeout, Duration},
};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(12);
const PROCESS_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

#[derive(Debug, Clone, Error)]
pub enum AppServerError {
    #[error("failed to spawn Codex app-server")]
    Spawn,
    #[error("Codex app-server stdin unavailable")]
    MissingStdin,
    #[error("Codex app-server stdout unavailable")]
    MissingStdout,
    #[error("Codex app-server stderr unavailable")]
    MissingStderr,
    #[error("{method} request timed out")]
    Timeout { method: String },
    #[error("app-server RPC error for {method} (code {code:?})")]
    Rpc {
        method: String,
        code: Option<i64>,
        method_not_found: bool,
    },
    #[error("app-server process exited before completing a request (code {code:?})")]
    ProcessExited {
        code: Option<i32>,
        transport_rejected: bool,
    },
    #[error("app-server stream closed")]
    StreamClosed,
    #[error("app-server protocol response was not valid JSON")]
    Protocol,
    #[error("app-server is closed")]
    Closed,
    #[error("app-server I/O failed")]
    Io,
    #[error("failed to serialize app-server request")]
    Serialize,
}

impl AppServerError {
    pub fn is_method_not_found(&self) -> bool {
        matches!(
            self,
            Self::Rpc {
                method_not_found: true,
                ..
            }
        )
    }

    fn requires_legacy_transport(&self) -> bool {
        matches!(
            self,
            Self::ProcessExited {
                transport_rejected: true,
                ..
            }
        )
    }

    fn needs_cleanup(&self) -> bool {
        matches!(
            self,
            Self::Timeout { .. }
                | Self::ProcessExited { .. }
                | Self::StreamClosed
                | Self::Protocol
                | Self::Closed
                | Self::Io
        )
    }

    pub fn is_transport_failure(&self) -> bool {
        self.needs_cleanup()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportMode {
    CurrentListenStdio,
    LegacyStdioFlag,
}

impl TransportMode {
    fn args(self) -> &'static [&'static str] {
        match self {
            Self::CurrentListenStdio => &["app-server", "--listen", "stdio://"],
            Self::LegacyStdioFlag => &["app-server", "--stdio"],
        }
    }
}

fn process_args(mode: TransportMode, diagnostic_cli_args: &[String]) -> Vec<String> {
    diagnostic_cli_args
        .iter()
        .cloned()
        .chain(mode.args().iter().map(|arg| (*arg).to_string()))
        .collect()
}

type PendingResponses = HashMap<u64, oneshot::Sender<Result<Value, AppServerError>>>;

struct StartupState {
    transport_rejected: AtomicBool,
}

impl StartupState {
    fn new() -> Self {
        Self {
            transport_rejected: AtomicBool::new(false),
        }
    }
}

struct TaskHandles {
    reader: Option<JoinHandle<()>>,
    stderr: Option<JoinHandle<()>>,
    monitor: Option<JoinHandle<()>>,
    ctrl_c: Option<JoinHandle<()>>,
}

pub struct AppServerClient {
    child: Arc<Mutex<Child>>,
    stdin: Arc<Mutex<ChildStdin>>,
    pending: Arc<Mutex<PendingResponses>>,
    notifications: broadcast::Sender<Value>,
    next_id: AtomicU64,
    tasks: Mutex<TaskHandles>,
    closed: Arc<AtomicBool>,
    shutdown_started: AtomicBool,
    pub transport_mode: TransportMode,
}

impl AppServerClient {
    pub async fn connect(codex_exe: impl AsRef<Path>) -> Result<(Self, Value), AppServerError> {
        Self::connect_inner(codex_exe.as_ref(), &[]).await
    }

    #[cfg(feature = "diagnostic-telemetry")]
    pub async fn connect_with_diagnostic_cli_args(
        codex_exe: impl AsRef<Path>,
        diagnostic_cli_args: Vec<String>,
    ) -> Result<(Self, Value), AppServerError> {
        Self::connect_inner(codex_exe.as_ref(), &diagnostic_cli_args).await
    }

    async fn connect_inner(
        codex_exe: &Path,
        diagnostic_cli_args: &[String],
    ) -> Result<(Self, Value), AppServerError> {
        let current = Self::spawn_with_mode(
            codex_exe,
            TransportMode::CurrentListenStdio,
            diagnostic_cli_args,
        )
        .await?;

        match current.initialize().await {
            Ok(initialize) => Ok((current, initialize)),
            Err(error) if error.requires_legacy_transport() => {
                let _ = current.shutdown().await;
                let legacy = Self::spawn_with_mode(
                    codex_exe,
                    TransportMode::LegacyStdioFlag,
                    diagnostic_cli_args,
                )
                .await?;
                match legacy.initialize().await {
                    Ok(initialize) => Ok((legacy, initialize)),
                    Err(error) => {
                        let _ = legacy.shutdown().await;
                        Err(error)
                    }
                }
            }
            Err(error) => {
                let _ = current.shutdown().await;
                Err(error)
            }
        }
    }

    async fn spawn_with_mode(
        codex_exe: &Path,
        mode: TransportMode,
        diagnostic_cli_args: &[String],
    ) -> Result<Self, AppServerError> {
        let mut command = Command::new(codex_exe);
        command
            .args(process_args(mode, diagnostic_cli_args))
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        #[cfg(windows)]
        command.creation_flags(CREATE_NO_WINDOW);

        let mut child = command.spawn().map_err(|_| AppServerError::Spawn)?;
        #[cfg(feature = "diagnostic-telemetry")]
        crate::diagnostic::spawned(child.id(), &format!("{mode:?}"));
        let stdin = child.stdin.take().ok_or(AppServerError::MissingStdin)?;
        let stdout = child.stdout.take().ok_or(AppServerError::MissingStdout)?;
        let stderr = child.stderr.take().ok_or(AppServerError::MissingStderr)?;

        let child = Arc::new(Mutex::new(child));
        #[cfg(feature = "diagnostic-telemetry")]
        if crate::diagnostic::active() {
            let sampled_child = child.clone();
            tokio::spawn(async move {
                sleep(Duration::from_millis(250)).await;
                let alive = sampled_child
                    .lock()
                    .await
                    .try_wait()
                    .ok()
                    .map(|s| s.is_none());
                crate::diagnostic::sample_alive(alive);
            });
        }
        let stdin = Arc::new(Mutex::new(stdin));
        let pending: Arc<Mutex<PendingResponses>> = Arc::new(Mutex::new(HashMap::new()));
        let (notifications, _) = broadcast::channel(128);
        let closed = Arc::new(AtomicBool::new(false));
        let startup = Arc::new(StartupState::new());

        let reader_task = {
            let pending = pending.clone();
            let notifications = notifications.clone();
            let closed = closed.clone();
            tokio::spawn(async move {
                let mut lines = BufReader::new(stdout).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    let line = line.trim();
                    if line.is_empty() {
                        continue;
                    }

                    let value = match serde_json::from_str::<Value>(line) {
                        Ok(value) => value,
                        Err(_) => {
                            closed.store(true, Ordering::SeqCst);
                            fail_pending(&pending, AppServerError::Protocol).await;
                            return;
                        }
                    };

                    if let Some(id) = request_id(&value) {
                        #[cfg(feature = "diagnostic-telemetry")]
                        if id == 1 {
                            crate::diagnostic::record("INITIALIZE_RESPONSE_RECEIVED", true);
                        }
                        if let Some(sender) = pending.lock().await.remove(&id) {
                            let _ = sender.send(Ok(value));
                        } else if value.get("method").is_some() {
                            let _ = notifications.send(value);
                        }
                    } else if value.get("method").is_some() {
                        let _ = notifications.send(value);
                    }
                }

                closed.store(true, Ordering::SeqCst);
                fail_pending(&pending, AppServerError::StreamClosed).await;
            })
        };

        let stderr_task = {
            let startup = startup.clone();
            tokio::spawn(async move {
                #[cfg(feature = "diagnostic-telemetry")]
                if crate::diagnostic::active() {
                    let mut reader = BufReader::new(stderr);
                    let mut bytes = Vec::new();
                    loop {
                        bytes.clear();
                        match reader.read_until(b'\n', &mut bytes).await {
                            Ok(0) => {
                                crate::diagnostic::record("STDERR_COMPLETE", true);
                                break;
                            }
                            Ok(_) => {
                                crate::diagnostic::stderr(&bytes);
                                if looks_like_transport_rejection(&String::from_utf8_lossy(&bytes))
                                {
                                    startup.transport_rejected.store(true, Ordering::SeqCst);
                                }
                            }
                            Err(_) => break,
                        }
                    }
                    return;
                }
                let mut lines = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    if looks_like_transport_rejection(&line) {
                        startup.transport_rejected.store(true, Ordering::SeqCst);
                    }
                    // stderr is intentionally drained but never retained or logged.
                }
            })
        };

        let monitor_task = {
            let child = child.clone();
            let pending = pending.clone();
            let closed = closed.clone();
            let startup = startup.clone();
            tokio::spawn(async move {
                loop {
                    if closed.load(Ordering::SeqCst) {
                        return;
                    }

                    let status = {
                        let mut child = child.lock().await;
                        child.try_wait()
                    };

                    match status {
                        Ok(Some(status)) => {
                            // Give the stderr drainer a brief chance to classify an unsupported
                            // current transport before deciding whether legacy fallback applies.
                            sleep(Duration::from_millis(50)).await;
                            let transport_rejected =
                                startup.transport_rejected.load(Ordering::SeqCst);
                            closed.store(true, Ordering::SeqCst);
                            fail_pending(
                                &pending,
                                AppServerError::ProcessExited {
                                    code: status.code(),
                                    transport_rejected,
                                },
                            )
                            .await;
                            return;
                        }
                        Ok(None) => sleep(Duration::from_millis(25)).await,
                        Err(_) => {
                            closed.store(true, Ordering::SeqCst);
                            fail_pending(
                                &pending,
                                AppServerError::ProcessExited {
                                    code: None,
                                    transport_rejected: false,
                                },
                            )
                            .await;
                            return;
                        }
                    }
                }
            })
        };

        let ctrl_c_task = {
            let child = child.clone();
            let stdin = stdin.clone();
            let pending = pending.clone();
            let closed = closed.clone();
            tokio::spawn(async move {
                if tokio::signal::ctrl_c().await.is_err() {
                    return;
                }

                closed.store(true, Ordering::SeqCst);
                {
                    let mut stdin = stdin.lock().await;
                    let _ = stdin.shutdown().await;
                }
                {
                    let mut child = child.lock().await;
                    let _ = child.start_kill();
                }
                fail_pending(&pending, AppServerError::Closed).await;
            })
        };

        Ok(Self {
            child,
            stdin,
            pending,
            notifications,
            next_id: AtomicU64::new(1),
            tasks: Mutex::new(TaskHandles {
                reader: Some(reader_task),
                stderr: Some(stderr_task),
                monitor: Some(monitor_task),
                ctrl_c: Some(ctrl_c_task),
            }),
            closed,
            shutdown_started: AtomicBool::new(false),
            transport_mode: mode,
        })
    }

    pub async fn initialize(&self) -> Result<Value, AppServerError> {
        #[cfg(feature = "diagnostic-telemetry")]
        if crate::diagnostic::active() {
            let mut child = self.child.lock().await;
            match child.try_wait() {
                Ok(status) => {
                    crate::diagnostic::record("ALIVE_BEFORE_INITIALIZE", status.is_none());
                    if let Some(status) = status {
                        crate::diagnostic::record("CHILD_EXITED", true);
                        crate::diagnostic::record("EXIT_CODE", json!(status.code()));
                    }
                }
                Err(_) => {}
            }
        }
        let response = self
            .request(
                "initialize",
                Some(json!({
                    "clientInfo": {
                        "name": "codex_flow",
                        "title": "Codex Flow",
                        "version": env!("CARGO_PKG_VERSION")
                    },
                    "capabilities": {
                        "experimentalApi": false
                    }
                })),
            )
            .await?;
        self.notify("initialized", None).await?;
        #[cfg(feature = "diagnostic-telemetry")]
        crate::diagnostic::record("INITIALIZED_SENT", true);
        Ok(response)
    }

    pub async fn account_read(&self) -> Result<Value, AppServerError> {
        self.request("account/read", Some(json!({ "refreshToken": false })))
            .await
    }

    pub async fn rate_limits_read(&self) -> Result<Value, AppServerError> {
        self.request("account/rateLimits/read", None).await
    }

    pub async fn usage_read(&self) -> Result<Value, AppServerError> {
        self.request("account/usage/read", None).await
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Value> {
        self.notifications.subscribe()
    }

    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }

    pub async fn request(
        &self,
        method: &str,
        params: Option<Value>,
    ) -> Result<Value, AppServerError> {
        if self.closed.load(Ordering::SeqCst) {
            return Err(AppServerError::Closed);
        }

        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (sender, receiver) = oneshot::channel();
        self.pending.lock().await.insert(id, sender);

        let mut message = json!({ "id": id, "method": method });
        if let Some(params) = params {
            message["params"] = params;
        }

        if let Err(error) = self.write_line(&message).await {
            self.pending.lock().await.remove(&id);
            let _ = self.shutdown().await;
            return Err(error);
        }

        let response = match timeout(REQUEST_TIMEOUT, receiver).await {
            Ok(Ok(response)) => response,
            Ok(Err(_)) => {
                self.pending.lock().await.remove(&id);
                let error = AppServerError::StreamClosed;
                let _ = self.shutdown().await;
                return Err(error);
            }
            Err(_) => {
                self.pending.lock().await.remove(&id);
                let error = AppServerError::Timeout {
                    method: method.to_string(),
                };
                let _ = self.shutdown().await;
                return Err(error);
            }
        };

        let envelope = match response {
            Ok(envelope) => envelope,
            Err(error) => {
                if error.needs_cleanup() {
                    let _ = self.shutdown().await;
                }
                return Err(error);
            }
        };

        if let Some(error) = envelope.get("error") {
            let code = error.get("code").and_then(Value::as_i64);
            #[cfg(feature = "diagnostic-telemetry")]
            if method == "account/rateLimits/read" {
                crate::diagnostic::rate_limits_rpc_error(
                    code,
                    error.get("message").and_then(Value::as_str),
                );
            }
            let method_not_found = code == Some(-32601)
                || error
                    .get("message")
                    .and_then(Value::as_str)
                    .map(|message| {
                        let message = message.to_ascii_lowercase();
                        message.contains("method not found") || message.contains("unknown method")
                    })
                    .unwrap_or(false);
            return Err(AppServerError::Rpc {
                method: method.to_string(),
                code,
                method_not_found,
            });
        }

        envelope
            .get("result")
            .cloned()
            .ok_or(AppServerError::Protocol)
    }

    pub async fn notify(&self, method: &str, params: Option<Value>) -> Result<(), AppServerError> {
        let mut message = json!({ "method": method });
        if let Some(params) = params {
            message["params"] = params;
        }
        match self.write_line(&message).await {
            Ok(()) => Ok(()),
            Err(error) => {
                let _ = self.shutdown().await;
                Err(error)
            }
        }
    }

    async fn write_line(&self, value: &Value) -> Result<(), AppServerError> {
        let mut stdin = self.stdin.lock().await;
        let mut bytes = serde_json::to_vec(value).map_err(|_| AppServerError::Serialize)?;
        bytes.push(b'\n');
        #[cfg(feature = "diagnostic-telemetry")]
        if value.get("method").and_then(Value::as_str) == Some("initialize") {
            crate::diagnostic::initialize_wire(&bytes);
            crate::diagnostic::record("INITIALIZE_WRITE_ATTEMPTED", true);
        }
        stdin
            .write_all(&bytes)
            .await
            .map_err(|_| AppServerError::Io)?;
        stdin.flush().await.map_err(|_| AppServerError::Io)?;
        #[cfg(feature = "diagnostic-telemetry")]
        if value.get("method").and_then(Value::as_str) == Some("initialize") {
            crate::diagnostic::record("INITIALIZE_WRITE_COMPLETED", true);
        }
        Ok(())
    }

    pub async fn shutdown(&self) -> Result<(), AppServerError> {
        if self.shutdown_started.swap(true, Ordering::SeqCst) {
            return Ok(());
        }
        self.closed.store(true, Ordering::SeqCst);

        #[cfg(feature = "diagnostic-telemetry")]
        crate::diagnostic::record("SHUTDOWN_STARTED", true);
        {
            let mut tasks = self.tasks.lock().await;
            if let Some(task) = tasks.reader.take() {
                task.abort();
            }
            if let Some(task) = tasks.stderr.take() {
                task.abort();
            }
            if let Some(task) = tasks.monitor.take() {
                task.abort();
            }
            if let Some(task) = tasks.ctrl_c.take() {
                task.abort();
            }
        }

        {
            let mut stdin = self.stdin.lock().await;
            let _ = stdin.shutdown().await;
        }

        {
            let mut child = self.child.lock().await;
            #[cfg(feature = "diagnostic-telemetry")]
            if crate::diagnostic::active() {
                let status = child.try_wait().ok().flatten();
                crate::diagnostic::record("EXIT_OBSERVED_BEFORE_CLEANUP", status.is_some());
                if let Some(status) = status {
                    crate::diagnostic::record("EXIT_CODE", json!(status.code()));
                }
            }
            let _ = child.start_kill();
            let _ = timeout(PROCESS_SHUTDOWN_TIMEOUT, child.wait()).await;
            #[cfg(feature = "diagnostic-telemetry")]
            if crate::diagnostic::active() {
                if let Ok(Some(status)) = child.try_wait() {
                    crate::diagnostic::record("CHILD_EXITED", true);
                    crate::diagnostic::record("EXIT_CODE", json!(status.code()));
                }
            }
        }

        fail_pending(&self.pending, AppServerError::Closed).await;
        Ok(())
    }
}

impl Drop for AppServerClient {
    fn drop(&mut self) {
        self.closed.store(true, Ordering::SeqCst);
        if let Ok(mut tasks) = self.tasks.try_lock() {
            if let Some(task) = tasks.reader.take() {
                task.abort();
            }
            if let Some(task) = tasks.stderr.take() {
                task.abort();
            }
            if let Some(task) = tasks.monitor.take() {
                task.abort();
            }
            if let Some(task) = tasks.ctrl_c.take() {
                task.abort();
            }
        }
        if let Ok(mut child) = self.child.try_lock() {
            let _ = child.start_kill();
        }
    }
}

async fn fail_pending(pending: &Arc<Mutex<PendingResponses>>, error: AppServerError) {
    let waiters = {
        let mut pending = pending.lock().await;
        std::mem::take(&mut *pending)
    };
    for (_, sender) in waiters {
        let _ = sender.send(Err(error.clone()));
    }
}

fn request_id(value: &Value) -> Option<u64> {
    match value.get("id") {
        Some(Value::Number(id)) => id.as_u64(),
        Some(Value::String(id)) => id.parse::<u64>().ok(),
        _ => None,
    }
}

fn looks_like_transport_rejection(line: &str) -> bool {
    let line = line.to_ascii_lowercase();
    [
        "unexpected argument",
        "unknown option",
        "unrecognized option",
        "found argument '--listen'",
        "no such option",
        "invalid value 'stdio://'",
    ]
    .iter()
    .any(|fragment| line.contains(fragment))
}

#[cfg(test)]
mod tests {
    use super::{process_args, TransportMode};

    #[test]
    fn production_process_args_remain_unchanged() {
        assert_eq!(
            process_args(TransportMode::CurrentListenStdio, &[]),
            ["app-server", "--listen", "stdio://"]
        );
    }

    #[cfg(feature = "diagnostic-telemetry")]
    #[test]
    fn diagnostic_cli_args_precede_the_app_server_subcommand() {
        let diagnostic = vec!["-c".to_string(), "features.plugins=false".to_string()];
        assert_eq!(
            process_args(TransportMode::CurrentListenStdio, &diagnostic),
            [
                "-c",
                "features.plugins=false",
                "app-server",
                "--listen",
                "stdio://",
            ]
        );
    }
}
