use anyhow::{Context, Result};
use codex_flow_lib::core::{AppCore, CoreConfig, CoreState};
use serde_json::{json, Value};
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

fn project_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("src-tauri has project parent")
        .to_path_buf()
}

fn locate_codex() -> Option<PathBuf> {
    if let Some(path) = env::var_os("CODEX_FLOW_CODEX_BIN") {
        let path = PathBuf::from(path);
        return path.exists().then_some(path);
    }

    if let Some(local_app_data) = env::var_os("LOCALAPPDATA") {
        let path = PathBuf::from(local_app_data)
            .join("Programs")
            .join("OpenAI")
            .join("Codex")
            .join("bin")
            .join("codex.exe");
        if path.exists() {
            return Some(path);
        }
    }

    #[cfg(windows)]
    let locator = "where.exe";
    #[cfg(not(windows))]
    let locator = "which";

    let output = Command::new(locator).arg("codex").output().ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.exists())
}

fn codex_version(path: Option<&Path>) -> String {
    path.and_then(|path| Command::new(path).arg("--version").output().ok())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|version| !version.is_empty())
        .unwrap_or_else(|| "unknown".into())
}

fn compact_state(state: &CoreState) -> Value {
    json!({
        "connection": state.connection,
        "error": state.error,
        "account": state.account.as_ref().map(|account| json!({
            "authenticated": account.authenticated,
            "requiresOpenaiAuth": account.requires_openai_auth,
            "accountType": account.account_type,
            "planType": account.plan_type,
        })),
        "ordinaryUsageAllowed": state.ordinary_usage_allowed,
        "windows": state.windows.iter().map(|window| json!({
            "kind": window.kind,
            "durationMinutes": window.duration_minutes,
            "usedPercent": window.used_percent,
            "remainingPercent": window.remaining_percent,
            "hasReset": window.resets_at.is_some(),
        })).collect::<Vec<_>>(),
        "stale": state.stale,
        "hasLastSuccess": state.last_success_at.is_some(),
    })
}

fn has_blocking_error(state: &CoreState) -> bool {
    state.error.is_some() || state.stale || state.windows.is_empty()
}

#[tokio::main]
async fn main() -> Result<()> {
    let root = project_root();
    let report_path = root.join("reports").join("gate1-probe.md");
    fs::create_dir_all(report_path.parent().context("report directory")?)?;

    let codex = locate_codex();
    let core = AppCore::new(CoreConfig {
        codex_executable: codex.clone(),
    });

    let started = core.start().await;
    let client_live_after_start = core.has_live_client().await;
    let notification_task_after_start = core.has_notification_task().await;

    let refreshed = if client_live_after_start {
        Some(core.refresh().await)
    } else {
        None
    };
    let client_live_after_refresh = core.has_live_client().await;

    core.shutdown().await;
    let client_live_after_shutdown = core.has_live_client().await;
    let notification_task_after_shutdown = core.has_notification_task().await;

    let status = if !has_blocking_error(&started)
        && client_live_after_start
        && notification_task_after_start
        && refreshed
            .as_ref()
            .map(|state| !has_blocking_error(state))
            .unwrap_or(false)
        && client_live_after_refresh
        && !client_live_after_shutdown
        && !notification_task_after_shutdown
    {
        "PASS"
    } else {
        "HOLD"
    };

    let mut report = format!("# Codex Flow Gate 1 Probe\n\nStatus: **{status}**\n\n");
    report.push_str("## Redacted evidence\n\n");
    report.push_str(&format!(
        "- Codex executable: `{}`\n",
        codex
            .as_deref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "<unavailable>".into())
    ));
    report.push_str(&format!(
        "- Codex version: `{}`\n",
        codex_version(codex.as_deref())
    ));
    report.push_str(if client_live_after_start {
        "- initialize / initialized: `completed by AppServerClient::connect`\n"
    } else {
        "- initialize / initialized: `not completed; app-server closed before the retained client was available`\n"
    });
    report.push_str(&format!(
        "- account/read and account/rateLimits/read: `{}`\n",
        if !has_blocking_error(&started) {
            "state populated"
        } else {
            "not confirmed"
        }
    ));
    report.push_str(if client_live_after_start {
        "- account/usage/read: `attempted as optional capability; response not retained by core`\n"
    } else {
        "- account/usage/read: `not attempted because app-server was unavailable`\n"
    });
    report.push_str(&format!(
        "- long-running client after start: `{client_live_after_start}`; notification task: `{notification_task_after_start}`\n"
    ));
    report.push_str(&format!(
        "- same-core refresh client retained: `{client_live_after_refresh}`\n"
    ));
    report.push_str(&format!(
        "- owned client after shutdown: `{client_live_after_shutdown}`; notification task after shutdown: `{notification_task_after_shutdown}`\n"
    ));
    report.push_str(&format!(
        "- start state: `{}`\n",
        serde_json::to_string(&compact_state(&started))?
    ));
    if let Some(refreshed) = refreshed {
        report.push_str(&format!(
            "- refresh state: `{}`\n",
            serde_json::to_string(&compact_state(&refreshed))?
        ));
    }
    report.push_str(
        "\n## Privacy\n\nThe probe stores only executable metadata, account status, normalized quota metadata and lifecycle booleans. It does not read `auth.json`, retain tokens/cookies/auth headers, or store chats, prompts or source content.\n",
    );

    fs::write(&report_path, report)?;
    println!("Gate 1 report: {}", report_path.display());
    println!("Gate 1 status: {status}");

    if status == "PASS" {
        Ok(())
    } else {
        anyhow::bail!("Gate 1 HOLD")
    }
}
