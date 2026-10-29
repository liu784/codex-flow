//! Opt-in, process-local observer for one diagnostic run. Never stores stderr text.
use serde_json::{json, Value};
use std::sync::{Mutex, OnceLock};

#[link(name = "bcrypt")]
extern "system" {
    fn BCryptOpenAlgorithmProvider(
        out: *mut usize,
        algorithm: *const u16,
        provider: *const u16,
        flags: u32,
    ) -> i32;
    fn BCryptCreateHash(
        algorithm: usize,
        out: *mut usize,
        object: *mut u8,
        object_len: u32,
        secret: *mut u8,
        secret_len: u32,
        flags: u32,
    ) -> i32;
    fn BCryptHashData(hash: usize, bytes: *const u8, len: u32, flags: u32) -> i32;
    fn BCryptFinishHash(hash: usize, bytes: *mut u8, len: u32, flags: u32) -> i32;
    fn BCryptDestroyHash(hash: usize) -> i32;
    fn BCryptCloseAlgorithmProvider(algorithm: usize, flags: u32) -> i32;
}

struct Digest {
    algorithm: usize,
    hash: usize,
}
impl Digest {
    fn new() -> Result<Self, &'static str> {
        let mut value = Self {
            algorithm: 0,
            hash: 0,
        };
        let name: Vec<u16> = "SHA256\0".encode_utf16().collect();
        unsafe {
            if BCryptOpenAlgorithmProvider(&mut value.algorithm, name.as_ptr(), std::ptr::null(), 0)
                < 0
                || BCryptCreateHash(
                    value.algorithm,
                    &mut value.hash,
                    std::ptr::null_mut(),
                    0,
                    std::ptr::null_mut(),
                    0,
                    0,
                ) < 0
            {
                return Err("DIAGNOSTIC_HASH_INIT_FAILED");
            }
        }
        Ok(value)
    }
    fn update(&mut self, bytes: &[u8]) -> bool {
        unsafe { BCryptHashData(self.hash, bytes.as_ptr(), bytes.len() as u32, 0) >= 0 }
    }
    fn finish(&mut self) -> Option<String> {
        let mut output = [0u8; 32];
        if unsafe { BCryptFinishHash(self.hash, output.as_mut_ptr(), 32, 0) } < 0 {
            return None;
        }
        Some(output.iter().map(|b| format!("{b:02X}")).collect())
    }
}
impl Drop for Digest {
    fn drop(&mut self) {
        unsafe {
            if self.hash != 0 {
                BCryptDestroyHash(self.hash);
            }
            if self.algorithm != 0 {
                BCryptCloseAlgorithmProvider(self.algorithm, 0);
            }
        }
    }
}

struct Observer {
    values: Value,
    digest: Digest,
    hash_valid: bool,
    stderr_length: usize,
}
static OBSERVER: OnceLock<Mutex<Observer>> = OnceLock::new();

pub fn enable() -> Result<(), &'static str> {
    OBSERVER
        .set(Mutex::new(Observer {
            values: json!({
                "PROCESS_START": false, "PID": null, "ALIVE_AFTER_250MS": null,
                "ALIVE_BEFORE_INITIALIZE": null, "INITIALIZE_WRITE_ATTEMPTED": false,
                "INITIALIZE_WRITE_COMPLETED": false, "INITIALIZE_RESPONSE_RECEIVED": false,
                "INITIALIZED_SENT": false, "CHILD_EXITED": null, "EXIT_CODE": null,
                "STDERR_CLASS": "UNKNOWN", "STDERR_COMPLETE": false,
                "SHUTDOWN_STARTED": false, "TRANSPORT_ATTEMPTS": 0
            }),
            digest: Digest::new()?,
            hash_valid: true,
            stderr_length: 0,
        }))
        .map_err(|_| "DIAGNOSTIC_ALREADY_ENABLED")
}
pub fn active() -> bool {
    OBSERVER.get().is_some()
}
fn kind(value: Option<&Value>) -> &'static str {
    match value {
        None => "omitted",
        Some(Value::Null) => "null",
        Some(Value::Bool(_)) => "boolean",
        Some(Value::Number(_)) => "number",
        Some(Value::String(_)) => "string",
        Some(Value::Array(_)) => "array",
        Some(Value::Object(_)) => "object",
    }
}
fn keys(value: &Value) -> Vec<String> {
    value
        .as_object()
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default()
}
fn wire_summary(bytes: &[u8]) -> Value {
    let newline_count = bytes.iter().rev().take_while(|b| **b == b'\n').count();
    let parsed = serde_json::from_slice::<Value>(bytes);
    let mut digest = Digest::new().ok();
    let sha = digest
        .as_mut()
        .and_then(|d| if d.update(bytes) { d.finish() } else { None });
    let mut out = json!({"INITIALIZE_LINE_LENGTH": bytes.len(), "INITIALIZE_LINE_SHA256": sha,
        "TRAILING_NEWLINE_COUNT": newline_count,
        "CONTENT_LENGTH_HEADER_PRESENT": bytes.starts_with(b"Content-Length:"),
        "WIRE_SCHEMA_MATCH": false});
    let Ok(value) = parsed else {
        out["FIRST_MISMATCH"] = json!("invalid JSON");
        return out;
    };
    let params = &value["params"];
    let client = &params["clientInfo"];
    let caps = &params["capabilities"];
    for (name, v) in [
        ("ID_TYPE", value.get("id")),
        ("METHOD_TYPE", value.get("method")),
        ("PARAMS_TYPE", value.get("params")),
        ("CLIENT_INFO_TYPE", params.get("clientInfo")),
        ("CLIENT_NAME_TYPE", client.get("name")),
        ("CLIENT_TITLE_TYPE", client.get("title")),
        ("CLIENT_VERSION_TYPE", client.get("version")),
        ("CAPABILITIES_TYPE", params.get("capabilities")),
        ("EXPERIMENTAL_API_TYPE", caps.get("experimentalApi")),
    ] {
        out[name] = json!(kind(v));
    }
    out["TOP_LEVEL_KEYS"] = json!(keys(&value));
    out["PARAM_KEYS"] = json!(keys(params));
    out["CLIENT_INFO_KEYS"] = json!(keys(client));
    out["CAPABILITY_KEYS"] = json!(keys(caps));
    out["EXPERIMENTAL_API_VALUE"] = json!(caps.get("experimentalApi").and_then(Value::as_bool));
    out["JSONRPC_FIELD_PRESENT"] = json!(value.get("jsonrpc").is_some());
    let failure = if !value.is_object() {
        "top-level expected object"
    } else if !(value["id"].is_string() || value["id"].is_u64() || value["id"].is_i64()) {
        "id expected string/integer"
    } else if value["method"] != "initialize" {
        "method expected initialize"
    } else if !params.is_object() {
        "params expected object"
    } else if !client.is_object() {
        "clientInfo expected object"
    } else if !client["name"].is_string() {
        "clientInfo.name expected string"
    } else if !client["version"].is_string() {
        "clientInfo.version expected string"
    } else if client
        .get("title")
        .is_some_and(|v| !v.is_null() && !v.is_string())
    {
        "clientInfo.title expected string/null/omitted"
    } else if params
        .get("capabilities")
        .is_some_and(|v| !v.is_null() && !v.is_object())
    {
        "capabilities expected object/null/omitted"
    } else if caps.get("experimentalApi").is_some_and(|v| !v.is_boolean()) {
        "experimentalApi expected boolean"
    } else if newline_count != 1 || bytes[..bytes.len() - 1].contains(&b'\n') {
        "JSONL expected one trailing newline"
    } else {
        "NONE"
    };
    out["FIRST_MISMATCH"] = json!(failure);
    out["WIRE_SCHEMA_MATCH"] = json!(failure == "NONE");
    out
}
pub fn initialize_wire(bytes: &[u8]) {
    if active() {
        record("INITIALIZE_WIRE", wire_summary(bytes));
    }
}
pub fn sample_alive(alive: Option<bool>) {
    if let Some(observer) = OBSERVER.get() {
        if let Ok(mut state) = observer.lock() {
            state.values["ALIVE_AFTER_250MS"] = json!(alive);
            state.values["SAMPLE_250MS_AFTER_CLEANUP_STARTED"] =
                state.values["SHUTDOWN_STARTED"].clone();
        }
    }
}
pub fn record(key: &str, value: impl Into<Value>) {
    if let Some(observer) = OBSERVER.get() {
        if let Ok(mut state) = observer.lock() {
            state.values[key] = value.into();
        }
    }
}
pub fn spawned(pid: Option<u32>, transport: &str) {
    if let Some(observer) = OBSERVER.get() {
        if let Ok(mut state) = observer.lock() {
            let attempts = state.values["TRANSPORT_ATTEMPTS"].as_u64().unwrap_or(0) + 1;
            state.values["TRANSPORT_ATTEMPTS"] = json!(attempts);
            state.values["PROCESS_START"] = json!(true);
            state.values["PID"] = json!(pid);
            state.values["TRANSPORT"] = json!(transport);
        }
    }
}

fn rate_limit_error_summary(message: Option<&str>) -> Value {
    let message = message.unwrap_or_default();
    let text = message.trim().to_ascii_lowercase();
    let message_sha256 = Digest::new().ok().and_then(|mut digest| {
        if digest.update(message.as_bytes()) {
            digest.finish()
        } else {
            None
        }
    });
    let (backend_host, non_default_port) = extract_http_host(message)
        .map(|(host, non_default)| (host, if non_default { "YES" } else { "NO" }.to_string()))
        .unwrap_or_else(|| ("UNKNOWN".to_string(), "UNKNOWN".to_string()));
    let route = if text.contains("wham/usage") {
        "WHAM_USAGE"
    } else if text.contains("api/codex/usage") {
        "API_CODEX_USAGE"
    } else {
        "UNKNOWN"
    };
    let content_type = if text.contains("application/json") {
        "JSON"
    } else if text.contains("text/html") {
        "HTML"
    } else if text.contains("text/plain") {
        "TEXT"
    } else if text.contains("empty body") || text.contains("body was empty") {
        "EMPTY"
    } else {
        "OTHER"
    };
    let http_status = text.split("failed:").nth(1).and_then(|tail| {
        tail.split(|character: char| !character.is_ascii_digit())
            .find_map(|part| {
                part.parse::<u16>()
                    .ok()
                    .filter(|status| (100..600).contains(status))
            })
    });
    let transport_error = if text.contains("dns")
        || text.contains("name resolution")
        || text.contains("failed to lookup")
    {
        "DNS"
    } else if text.contains("certificate") || text.contains("cert") || text.contains("trust") {
        "TLS_CERTIFICATE"
    } else if text.contains("tls") || text.contains("handshake") {
        "TLS_HANDSHAKE"
    } else if text.contains("timeout") || text.contains("timed out") {
        "TIMEOUT"
    } else if text.contains("proxy") || text.contains("tunnel") {
        "PROXY"
    } else if text.contains("connection refused")
        || text.contains("connect error")
        || text.contains("connection reset")
    {
        "CONNECTION"
    } else {
        "OTHER"
    };
    let failure_class = if text == "failed to fetch codex rate limits: no snapshots returned" {
        "EMPTY_SNAPSHOTS"
    } else if text.contains("get ") && text.contains("/usage") && http_status.is_some() {
        "BACKEND_HTTP_ERROR"
    } else if text.contains("decode error for") && text.contains("/usage") {
        "BACKEND_DECODE_ERROR"
    } else if text.contains("error sending request")
        || text.contains("connection")
        || text.contains("connect")
        || text.contains("dns")
        || text.contains("name resolution")
        || text.contains("tls")
        || text.contains("certificate")
        || text.contains("timeout")
        || text.contains("timed out")
        || text.contains("proxy")
    {
        "BACKEND_TRANSPORT_ERROR"
    } else {
        "UNKNOWN"
    };

    json!({
        "RATE_LIMIT_FAILURE_CLASS": failure_class,
        "BACKEND_ROUTE": route,
        "BACKEND_HOST": backend_host,
        "NON_DEFAULT_PORT": non_default_port,
        "ERROR_MESSAGE_LENGTH": message.len(),
        "ERROR_MESSAGE_SHA256": message_sha256,
        "HTTP_STATUS": http_status,
        "CONTENT_TYPE_CLASS": content_type,
        "TRANSPORT_ERROR_CLASS": if failure_class == "BACKEND_TRANSPORT_ERROR" {
            transport_error
        } else {
            "OTHER"
        },
        "EMPTY_SNAPSHOTS": if failure_class == "EMPTY_SNAPSHOTS" { "YES" } else { "NO" }
    })
}

fn extract_http_host(message: &str) -> Option<(String, bool)> {
    let lower = message.to_ascii_lowercase();
    let https = lower.find("https://").map(|index| (index, 8, 443u16));
    let http = lower.find("http://").map(|index| (index, 7, 80u16));
    let (index, scheme_length, default_port) = match (https, http) {
        (Some(left), Some(right)) => {
            if left.0 <= right.0 {
                left
            } else {
                right
            }
        }
        (Some(found), None) | (None, Some(found)) => found,
        (None, None) => return None,
    };
    let authority = &message[index + scheme_length..];
    let end = authority
        .find(|character: char| {
            character.is_whitespace()
                || matches!(
                    character,
                    '/' | '?' | '#' | ')' | '>' | '"' | '\'' | ',' | ';'
                )
        })
        .unwrap_or(authority.len());
    let authority = authority[..end].rsplit('@').next()?;
    let (host, port) = if let Some(rest) = authority.strip_prefix('[') {
        let closing = rest.find(']')?;
        let host = &rest[..closing];
        let port = rest[closing + 1..]
            .strip_prefix(':')
            .and_then(|value| value.parse::<u16>().ok());
        (host, port)
    } else if authority.matches(':').count() == 1 {
        let (host, candidate_port) = authority.rsplit_once(':')?;
        let port = candidate_port.parse::<u16>().ok()?;
        (host, Some(port))
    } else {
        (authority, None)
    };
    if host.is_empty()
        || !host.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_' | ':')
        })
    {
        return None;
    }
    Some((
        host.to_ascii_lowercase(),
        port.is_some_and(|value| value != default_port),
    ))
}

pub fn rate_limits_rpc_error(code: Option<i64>, message: Option<&str>) {
    if active() {
        record("RATE_LIMITS_ERROR_CODE", json!(code));
        record("RATE_LIMITS_FAILURE", rate_limit_error_summary(message));
    }
}

fn classify(text: &str) -> &'static str {
    let text = text.to_ascii_lowercase();
    if text.contains("unexpected argument") || text.contains("unrecognized subcommand") {
        "COMMAND_USAGE_ERROR"
    } else if text.contains("sqlite") && (text.contains("error") || text.contains("failed")) {
        "SQLITE_STATE_ERROR"
    } else if text.contains("codex_home") && (text.contains("error") || text.contains("failed")) {
        "CODEX_HOME_ERROR"
    } else if text.contains("failed to load config") {
        "CONFIG_LOAD_ERROR"
    } else if text.contains("authentication failed") {
        "AUTH_STATE_ERROR"
    } else if text.contains("failed to start app-server") {
        "APP_SERVER_STARTUP_ERROR"
    } else {
        "UNKNOWN"
    }
}
pub fn stderr(bytes: &[u8]) {
    if let Some(observer) = OBSERVER.get() {
        if let Ok(mut state) = observer.lock() {
            state.stderr_length += bytes.len();
            state.hash_valid &= state.digest.update(bytes);
            let class = classify(&String::from_utf8_lossy(bytes));
            if class != "UNKNOWN" {
                state.values["STDERR_CLASS"] = json!(class);
            }
        }
    }
}
pub fn finish() -> Value {
    let mut state = OBSERVER
        .get()
        .expect("diagnostic enabled")
        .lock()
        .expect("diagnostic lock");
    state.values["STDERR_PRESENT"] = json!(state.stderr_length > 0);
    state.values["STDERR_LENGTH"] = json!(state.stderr_length);
    state.values["STDERR_SHA256"] = if state.hash_valid {
        json!(state.digest.finish())
    } else {
        Value::Null
    };
    state.values.clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wire_check_rejects_wrong_boolean_and_reports_only_structure() {
        let bytes = b"{\"id\":1,\"method\":\"initialize\",\"params\":{\"clientInfo\":{\"name\":\"private-name\",\"version\":\"v\"},\"capabilities\":{\"experimentalApi\":{}}}}\n";
        let report = wire_summary(bytes);
        assert_eq!(report["WIRE_SCHEMA_MATCH"], false);
        assert_eq!(report["FIRST_MISMATCH"], "experimentalApi expected boolean");
        assert_eq!(report["TRAILING_NEWLINE_COUNT"], 1);
        assert!(!report.to_string().contains("private-name"));
    }
    #[test]
    fn sha256_known_vector_and_safe_classes() {
        let mut hash = Digest::new().unwrap();
        assert!(hash.update(b"a"));
        assert!(hash.update(b"bc"));
        assert_eq!(
            hash.finish().unwrap(),
            "BA7816BF8F01CFEA414140DE5DAE2223B00361A396177A9CB410FF61F20015AD"
        );
        assert_eq!(classify("unclassified private text"), "UNKNOWN");
        assert_eq!(classify("error sqlite failed"), "SQLITE_STATE_ERROR");
    }

    #[test]
    fn rate_limit_errors_are_reduced_to_safe_categories() {
        let empty = rate_limit_error_summary(Some(
            "failed to fetch codex rate limits: no snapshots returned",
        ));
        assert_eq!(empty["RATE_LIMIT_FAILURE_CLASS"], "EMPTY_SNAPSHOTS");
        assert_eq!(empty["EMPTY_SNAPSHOTS"], "YES");

        let backend = rate_limit_error_summary(Some(
            "GET https://user:secret@example.invalid:8443/backend-api/wham/usage?private=query failed: 503; content-type: text/html; connection reset; private-body",
        ));
        assert_eq!(backend["RATE_LIMIT_FAILURE_CLASS"], "BACKEND_HTTP_ERROR");
        assert_eq!(backend["BACKEND_ROUTE"], "WHAM_USAGE");
        assert_eq!(backend["BACKEND_HOST"], "example.invalid");
        assert_eq!(backend["NON_DEFAULT_PORT"], "YES");
        assert_eq!(backend["HTTP_STATUS"], 503);
        assert_eq!(backend["CONTENT_TYPE_CLASS"], "HTML");
        assert_eq!(backend["TRANSPORT_ERROR_CLASS"], "OTHER");
        assert_eq!(backend["ERROR_MESSAGE_SHA256"].as_str().unwrap().len(), 64);
        assert!(!backend.to_string().contains("private-body"));
        assert!(!backend.to_string().contains("https://"));
        assert!(!backend.to_string().contains("user:secret"));
        assert!(!backend.to_string().contains("backend-api"));
        assert_eq!(
            extract_http_host("https://example.invalid:443/private"),
            Some(("example.invalid".to_string(), false))
        );
    }
}
