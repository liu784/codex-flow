//! Explicit opt-in diagnostic. Uses the product resolver, client and handshake.
use codex_flow_lib::{codex::client::AppServerClient, core::service::locate_codex, diagnostic};
use serde_json::{json, Value};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    time::{SystemTime, UNIX_EPOCH},
};

fn field(value: &Value, name: &str) -> Value {
    value.get(name).cloned().unwrap_or_else(|| json!("MISSING"))
}
fn quota_groups(response: &Value) -> Value {
    let mut groups = Vec::new();
    let mut index = 0;
    let mut add = |snapshot: &Value, source: String| {
        index += 1;
        for slot in ["primary", "secondary"] {
            let window = snapshot.get(slot).unwrap_or(&Value::Null);
            groups.push(json!({
                "GROUP_INDEX": index, "SOURCE_PATH": source, "SLOT": slot,
                "LIMIT_ID": field(snapshot, "limitId"),
                "WINDOW_DURATION_MINUTES": field(window, "windowDurationMins"),
                "USED_PERCENT": field(window, "usedPercent"),
                "REMAINING_PERCENT": field(window, "remainingPercent"),
                "RESET_AT": field(window, "resetsAt"),
                "ORDINARY_USAGE_ALLOWED": field(snapshot, "ordinaryUsageAllowed"),
                "WINDOW_PRESENT": !window.is_null(),
                "SOURCE_METHOD": "account/rateLimits/read"
            }));
        }
    };
    if let Some(snapshot) = response.get("rateLimits") {
        add(snapshot, "rateLimits".into());
    }
    if let Some(map) = response
        .get("rateLimitsByLimitId")
        .and_then(Value::as_object)
    {
        for (key, snapshot) in map {
            add(snapshot, format!("rateLimitsByLimitId.{key}"));
        }
    }
    json!(groups)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = std::env::args().collect::<Vec<_>>();
    let wire_only = args.iter().any(|arg| arg == "--wire-only");
    let plugins_disabled = args.iter().any(|arg| arg == "--plugins-disabled");
    let diagnostic_cli_args = if plugins_disabled {
        vec!["-c".to_string(), "features.plugins=false".to_string()]
    } else {
        Vec::new()
    };
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let parent = root.join("artifacts/app-server-diagnostics");
    fs::create_dir_all(&parent)?;
    let dir = parent.join(stamp.to_string());
    fs::create_dir(&dir)?;
    diagnostic::enable().map_err(anyhow::Error::msg)?;
    let mut report = json!({
        "WORKING_DIRECTORY": std::env::current_dir()?,
        "CODEX_HOME_PRESENT": std::env::var_os("CODEX_HOME").is_some(),
        "CODEX_SQLITE_HOME_PRESENT": std::env::var_os("CODEX_SQLITE_HOME").is_some(),
        "PRODUCT_PATH_BEHAVIOR_CHANGED": false,
        "ARGUMENTS": diagnostic_cli_args.iter().cloned().chain(
            ["app-server", "--listen", "stdio://"].into_iter().map(String::from)
        ).collect::<Vec<_>>(),
        "ACCOUNT_READ": "NOT_ATTEMPTED", "RATE_LIMITS_READ": "NOT_ATTEMPTED",
        "APP_SERVER_GROUPS": "UNKNOWN", "DIAGNOSTIC_MATCHES_PRODUCT": true
    });
    match locate_codex(None) {
        Err(_) => {
            report["ERROR"] = json!("CODEX_NOT_FOUND");
        }
        Ok(path) => {
            report["CODEX_PATH"] = json!(path);
            let connection = if diagnostic_cli_args.is_empty() {
                AppServerClient::connect(&path).await
            } else {
                AppServerClient::connect_with_diagnostic_cli_args(
                    &path,
                    diagnostic_cli_args.clone(),
                )
                .await
            };
            match connection {
                Err(error) => {
                    report["ERROR"] = json!(error.to_string());
                }
                Ok((client, _)) => {
                    if wire_only {
                        let _ = client.shutdown().await;
                    } else {
                        match client.account_read().await {
                            Ok(account) => {
                                report["ACCOUNT_READ"] = json!("PASS");
                                report["ACCOUNT_SUMMARY"] = json!({"HAS_ACCOUNT": account.get("account").is_some_and(|v| !v.is_null()),
                                "PLAN_TYPE": field(&account["account"], "planType")});
                                match client.rate_limits_read().await {
                                    Ok(quota) => {
                                        report["RATE_LIMITS_READ"] = json!("PASS");
                                        report["ORDINARY_USAGE_ALLOWED"] =
                                            field(&quota, "ordinaryUsageAllowed");
                                        report["APP_SERVER_GROUPS"] = quota_groups(&quota);
                                    }
                                    Err(error) => {
                                        report["RATE_LIMITS_READ"] = json!("HOLD");
                                        report["ERROR"] = json!(error.to_string());
                                    }
                                }
                            }
                            Err(error) => {
                                report["ACCOUNT_READ"] = json!("HOLD");
                                report["ERROR"] = json!(error.to_string());
                            }
                        }
                        let _ = client.shutdown().await;
                    }
                }
            }
        }
    }
    // Let the independent 250ms sample complete; never delay the handshake itself.
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    report["TELEMETRY"] = diagnostic::finish();
    report["RESULT"] = if report["RATE_LIMITS_READ"] == "PASS" {
        json!("PASS")
    } else {
        json!("HOLD")
    };
    if report["TELEMETRY"]["TRANSPORT_ATTEMPTS"]
        .as_u64()
        .unwrap_or(0)
        > 1
    {
        report["TELEMETRY_NOTE"] = json!("Product legacy fallback occurred; telemetry spans attempts and must not be treated as single-child stage proof");
    }
    let target = dir.join("diagnostic.json");
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&target)?;
    file.write_all(serde_json::to_string_pretty(&report)?.as_bytes())?;
    println!("DIAGNOSTIC_ARTIFACT={}", target.display());
    println!("DIAGNOSTIC_RESULT={}", report["RESULT"]);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn quota_summary_preserves_raw_fields_without_deriving_remaining() {
        let summary = quota_groups(
            &json!({"rateLimits":{"limitId":"x","primary":{"usedPercent":71,"windowDurationMins":10080,"resetsAt":123}}}),
        );
        assert_eq!(summary[0]["USED_PERCENT"], 71);
        assert_eq!(summary[0]["REMAINING_PERCENT"], "MISSING");
        assert_eq!(summary[0]["RESET_AT"], 123);
    }
}
