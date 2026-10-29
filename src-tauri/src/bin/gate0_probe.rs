use anyhow::{Context, Result};
use codex_flow_lib::{
    codex::{
        client::{AppServerClient, AppServerError},
        protocol::{summarize_account, GetAccountRateLimitsResponse, GetAccountResponse},
        redact::redact_json,
    },
    quota::normalize::{normalize_response, NormalizedQuotaWindow},
};
use serde_json::{json, Value};
use std::{
    env,
    path::{Path, PathBuf},
    process::Command,
};

fn project_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("src-tauri has project parent")
        .to_path_buf()
}

fn locate_codex() -> Result<PathBuf> {
    if let Ok(explicit) = env::var("CODEX_FLOW_CODEX_BIN") {
        let path = PathBuf::from(explicit);
        if path.exists() {
            return Ok(path);
        }
    }

    #[cfg(windows)]
    let output = Command::new("where.exe").arg("codex").output()?;
    #[cfg(not(windows))]
    let output = Command::new("which").arg("codex").output()?;

    if !output.status.success() {
        anyhow::bail!("CODEX_NOT_FOUND");
    }
    let first = String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .context("CODEX_NOT_FOUND")?;
    Ok(PathBuf::from(first))
}

fn codex_version(path: &Path) -> String {
    Command::new(path)
        .arg("--version")
        .output()
        .ok()
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "unknown".into())
}

fn json_shape(value: &Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, value)| (key.clone(), json_shape(value)))
                .collect(),
        ),
        Value::Array(values) => Value::Array(values.iter().take(2).map(json_shape).collect()),
        Value::Null => Value::Null,
        Value::Bool(_) => Value::String("<bool>".into()),
        Value::Number(_) => Value::String("<number>".into()),
        Value::String(_) => Value::String("<string>".into()),
    }
}

fn redacted_shape(value: &Value) -> Result<String> {
    Ok(serde_json::to_string(&json_shape(&redact_json(value)))?)
}

fn safe_label(value: Option<&str>) -> String {
    match value {
        Some(value)
            if !value.is_empty()
                && value.len() <= 64
                && value.chars().all(|character| {
                    character.is_ascii_alphanumeric() || "_-".contains(character)
                }) =>
        {
            format!("`{value}`")
        }
        Some(_) => "`<redacted>`".into(),
        None => "`<unavailable>`".into(),
    }
}

fn normalized_shape(windows: &[NormalizedQuotaWindow]) -> Value {
    Value::Array(
        windows
            .iter()
            .map(|window| {
                json!({
                    "sourceSlot": window.source_slot,
                    "kind": window.kind,
                    "durationMinutes": window.duration_minutes,
                    "usedPercent": window.used_percent,
                    "remainingPercent": window.remaining_percent,
                    "hasReset": window.resets_at.is_some(),
                })
            })
            .collect(),
    )
}

#[tokio::main]
async fn main() -> Result<()> {
    let tests_passed = env::args().any(|arg| arg == "--tests-passed");
    let root = project_root();
    let report_path = root.join("reports").join("gate0-probe.md");
    std::fs::create_dir_all(report_path.parent().unwrap())?;

    let mut evidence = Vec::new();
    let mut hold_reasons = Vec::new();

    let codex = match locate_codex() {
        Ok(path) => path,
        Err(error) => {
            hold_reasons.push(format!("Codex executable unavailable: {error:#}"));
            PathBuf::from("codex")
        }
    };
    let version = if codex.exists() || codex == PathBuf::from("codex") {
        codex_version(&codex)
    } else {
        "unknown".into()
    };
    evidence.push(format!("Codex executable: `{}`", codex.display()));
    evidence.push(format!("Codex version: `{version}`"));

    if hold_reasons.is_empty() {
        match AppServerClient::connect(&codex).await {
            Ok((client, initialize)) => {
                evidence.push(format!("Transport: `{:?}`", client.transport_mode));
                evidence.push(format!(
                    "Initialize structure: `{}`",
                    redacted_shape(&initialize)?
                ));
                evidence.push("initialized notification: `sent`".into());

                if hold_reasons.is_empty() {
                    match client.account_read().await {
                        Ok(raw) => {
                            evidence.push(format!(
                                "account/read structure: `{}`",
                                redacted_shape(&raw)?
                            ));
                            match serde_json::from_value::<GetAccountResponse>(raw) {
                                Ok(parsed) => {
                                    let state = summarize_account(&parsed);
                                    evidence.push(format!(
                                        "Account state: authenticated=`{}`; requiresOpenaiAuth=`{}`; accountType={}; plan={}",
                                        state.authenticated,
                                        state.requires_openai_auth,
                                        safe_label(state.account_type.as_deref()),
                                        safe_label(state.plan_type.as_deref()),
                                    ));
                                }
                                Err(error) => hold_reasons
                                    .push(format!("account/read shape mismatch: {error}")),
                            }
                        }
                        Err(error) => hold_reasons.push(format!("account/read failed: {error}")),
                    }

                    match client.rate_limits_read().await {
                        Ok(raw) => {
                            evidence.push(format!(
                                "account/rateLimits/read structure: `{}`",
                                redacted_shape(&raw)?
                            ));
                            match serde_json::from_value::<GetAccountRateLimitsResponse>(raw) {
                                Ok(parsed) => {
                                    let normalized = normalize_response(&parsed);
                                    evidence.push(format!(
                                        "Normalized windows: `{}`",
                                        serde_json::to_string(&normalized_shape(&normalized))?
                                    ));
                                    if normalized.is_empty() {
                                        hold_reasons.push("No quota windows were returned".into());
                                    }
                                }
                                Err(error) => hold_reasons.push(format!(
                                    "account/rateLimits/read shape mismatch: {error}"
                                )),
                            }
                        }
                        Err(error) => {
                            hold_reasons.push(format!("account/rateLimits/read failed: {error}"))
                        }
                    }

                    match client.usage_read().await {
                        Ok(raw) => evidence.push(format!(
                            "account/usage/read: supported; structure `{}`",
                            redacted_shape(&raw)?
                        )),
                        Err(error) if error.is_method_not_found() => {
                            evidence.push("account/usage/read: `unsupported`".into())
                        }
                        Err(AppServerError::Rpc { code, .. }) => evidence.push(format!(
                            "account/usage/read: non-fatal RPC error (code `{code:?}`)"
                        )),
                        Err(error) => {
                            evidence.push(format!("account/usage/read: non-fatal error `{error}`"))
                        }
                    }
                }

                let _ = client.shutdown().await;
            }
            Err(error) => hold_reasons.push(format!("App-server launch/handshake failed: {error}")),
        }
    }

    if !tests_passed {
        hold_reasons.push("Normalizer tests were not confirmed by the launcher".into());
    }

    let status = if hold_reasons.is_empty() {
        "PASS"
    } else {
        "HOLD"
    };
    let mut report =
        format!("# Codex Flow Gate 0 Probe\n\nStatus: **{status}**\n\n## Redacted evidence\n\n");
    for item in evidence {
        report.push_str(&format!("- {item}\n"));
    }
    if !hold_reasons.is_empty() {
        report.push_str("\n## Hold reasons\n\n");
        for reason in hold_reasons {
            report.push_str(&format!("- {reason}\n"));
        }
    }
    report.push_str(
        "\n## Privacy\n\nThe probe stores only redacted structure, account status and normalized quota metadata. It does not read `auth.json`, chats, prompts, or project source content.\n",
    );
    std::fs::write(&report_path, report)?;
    println!("Gate 0 report: {}", report_path.display());
    println!("Gate 0 status: {status}");

    if status == "PASS" {
        Ok(())
    } else {
        anyhow::bail!("Gate 0 HOLD")
    }
}
