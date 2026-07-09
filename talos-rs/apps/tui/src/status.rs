use std::path::PathBuf;

use talos_error::{TalosError, TalosResult};
use talos_sdk::{AuthSessionReadResponse, LlmReasoningEffort, SessionInfo, WorkspaceInfoResponse};
use talos_types::talos::{TokenUsage, TurnExecutionMetrics};

use crate::utils::{text_display_width, APP_VERSION};
use crate::TuiApp;

const CONTEXT_BAR_WIDTH: usize = 20;

/// Immutable status data prepared for `/status` rendering.
///
/// The snapshot owns all data needed by the renderer so the command handler
/// does not mix state collection with terminal presentation logic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TuiStatusSnapshot {
    model: StatusModel,
    agent_workspace: PathBuf,
    directory: PathBuf,
    account: String,
    session_id: Option<String>,
    runtime_mode: &'static str,
    connection_status: String,
    usage: TokenUsage,
    turn: StatusTurn,
    event_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StatusModel {
    id: String,
    provider: String,
    context_window: u32,
    reasoning: bool,
    reasoning_effort: LlmReasoningEffort,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StatusTurn {
    state: &'static str,
    metrics: Option<TurnExecutionMetrics>,
}

impl TuiStatusSnapshot {
    /// Builds a status snapshot from live TUI state and an optional service snapshot.
    ///
    /// Session data is authoritative when available. Missing model metadata is
    /// treated as a state synchronization error instead of being rendered as a
    /// placeholder value.
    pub(crate) fn from_app(
        app: &TuiApp,
        session: Option<&SessionInfo>,
        auth: &AuthSessionReadResponse,
        workspace: &WorkspaceInfoResponse,
    ) -> TalosResult<TuiStatusSnapshot> {
        let model_id = session
            .map(|session| session.model_ref.id.as_str())
            .unwrap_or(app.current_model.as_str());
        if model_id.trim().is_empty() {
            return Err(TalosError::ClientError(
                "status requires a loaded model".to_string(),
            ));
        }

        let model_info = app
            .available_model_info
            .iter()
            .find(|model| model.id == model_id)
            .ok_or_else(|| {
                TalosError::ClientError(format!("status model metadata missing for {model_id}"))
            })?;
        if model_info.context_window == 0 {
            return Err(TalosError::ClientError(format!(
                "status model context window missing for {model_id}"
            )));
        }

        let directory = session
            .and_then(|session| session.current_directory.clone())
            .unwrap_or_else(|| app.startup_directory.clone());
        let agent_workspace = PathBuf::from(&workspace.user_workspace_dir);
        let account = status_account(auth)
            .or_else(|| session.map(|session| session.user_id.clone()))
            .unwrap_or_else(current_account);
        let session_id = session
            .map(|session| session.id.clone())
            .or_else(|| app.session_id.clone());
        let mut usage = session
            .map(|session| session.usage.clone())
            .unwrap_or_else(|| app.session_usage.clone());
        if app.turn_metrics_live {
            if let Some(metrics) = &app.turn_metrics {
                usage.accumulate(&metrics.usage);
            }
        }

        Ok(TuiStatusSnapshot {
            model: StatusModel {
                id: model_info.id.clone(),
                provider: model_info.provider.clone(),
                context_window: model_info.context_window,
                reasoning: model_info.reasoning,
                reasoning_effort: app
                    .current_reasoning_effort
                    .filter(|_| model_info.reasoning && model_info.id == app.current_model)
                    .unwrap_or(model_info.reasoning_effort),
            },
            agent_workspace,
            directory,
            account,
            session_id,
            runtime_mode: "embedded",
            connection_status: app.connection_status.clone(),
            usage,
            turn: StatusTurn {
                state: if app.turn_metrics_live {
                    "live"
                } else {
                    "idle"
                },
                metrics: app.turn_metrics.clone(),
            },
            event_count: app.trace_event_count,
        })
    }
}

/// Renders the `/status` snapshot as a compact terminal card.
pub(crate) fn render_status_card(snapshot: &TuiStatusSnapshot) -> String {
    let mut rows = Vec::new();
    rows.push(format!(">_  Talos TUI ({APP_VERSION})"));
    rows.push(String::new());
    rows.push(status_row("Model:", &format_model(&snapshot.model)));
    rows.push(status_row(
        "Agent Workspace:",
        &format_path(&snapshot.agent_workspace),
    ));
    rows.push(status_row("Directory:", &format_path(&snapshot.directory)));
    rows.push(status_row("Account:", &snapshot.account));
    rows.push(status_row(
        "Session:",
        snapshot.session_id.as_deref().unwrap_or("none"),
    ));
    rows.push(status_row(
        "Runtime:",
        &format_runtime(snapshot.runtime_mode, &snapshot.connection_status),
    ));
    rows.push(status_row("Context:", &format_context_bar(snapshot)));
    rows.push(status_row(
        "Context window:",
        &format_context_window(snapshot),
    ));
    rows.push(status_row("Usage:", &format_usage(&snapshot.usage)));
    rows.push(status_row("Last turn:", &format_turn(&snapshot.turn)));
    rows.push(status_row("Events:", &snapshot.event_count.to_string()));

    let width = rows
        .iter()
        .map(|row| text_display_width(row))
        .max()
        .unwrap_or_default()
        .max(40);
    let border = "─".repeat(width + 2);
    let mut out = Vec::with_capacity(rows.len() + 2);
    out.push(format!("┌{border}┐"));
    for row in rows {
        out.push(format!(
            "│ {}{} │",
            row,
            " ".repeat(width.saturating_sub(text_display_width(&row)))
        ));
    }
    out.push(format!("└{border}┘"));
    out.join("\n")
}

fn status_row(label: &str, value: &str) -> String {
    format!("{label:<17} {value}")
}

fn format_model(model: &StatusModel) -> String {
    let reasoning = if model.reasoning {
        format!(
            "reasoning {}",
            format_reasoning_effort(model.reasoning_effort)
        )
    } else {
        "reasoning off".to_string()
    };
    format!(
        "{} ({}, {reasoning}, summaries auto)",
        model.id, model.provider
    )
}

fn format_reasoning_effort(effort: LlmReasoningEffort) -> &'static str {
    match effort {
        LlmReasoningEffort::Low => "low",
        LlmReasoningEffort::Medium => "medium",
        LlmReasoningEffort::High => "high",
        LlmReasoningEffort::XHigh => "xhigh",
    }
}

fn format_runtime(mode: &str, connection_status: &str) -> String {
    let dot = if connection_status == "connected" {
        "●"
    } else {
        "○"
    };
    format!("{mode} {dot} {connection_status}")
}

fn format_context_bar(snapshot: &TuiStatusSnapshot) -> String {
    let used = snapshot
        .usage
        .total_tokens
        .min(snapshot.model.context_window);
    let percent = used_percent(used, snapshot.model.context_window);
    let filled = filled_bar_cells(used, snapshot.model.context_window);
    let empty = CONTEXT_BAR_WIDTH.saturating_sub(filled);
    format!(
        "[{}{}] {percent}% used",
        "■".repeat(filled),
        "□".repeat(empty)
    )
}

fn format_context_window(snapshot: &TuiStatusSnapshot) -> String {
    let used = snapshot
        .usage
        .total_tokens
        .min(snapshot.model.context_window);
    let left = snapshot.model.context_window.saturating_sub(used);
    format!(
        "{} used / {} total · {} left",
        format_tokens(used),
        format_tokens(snapshot.model.context_window),
        format_tokens(left)
    )
}

fn format_usage(usage: &TokenUsage) -> String {
    let cache_read = usage.cache_read_input_tokens.unwrap_or_default();
    if cache_read > 0 {
        format!(
            "{} tokens · cache read {}",
            format_tokens(usage.total_tokens),
            format_tokens(cache_read)
        )
    } else {
        format!("{} tokens", format_tokens(usage.total_tokens))
    }
}

fn format_turn(turn: &StatusTurn) -> String {
    let Some(metrics) = &turn.metrics else {
        return format!("{} · no completed turn", turn.state);
    };
    let state = if turn.state == "live" { "live" } else { "last" };
    format!(
        "{state} · {} rounds · {} tools · {} ok · {} err · {}",
        metrics.model_rounds,
        metrics.tool_calls,
        metrics.tool_successes,
        metrics.tool_errors,
        format_duration(metrics.duration_ms)
    )
}

fn filled_bar_cells(used: u32, context_window: u32) -> usize {
    if used == 0 {
        return 0;
    }
    let filled = (used as usize)
        .saturating_mul(CONTEXT_BAR_WIDTH)
        .saturating_add(context_window as usize - 1)
        / context_window as usize;
    filled.clamp(1, CONTEXT_BAR_WIDTH)
}

fn used_percent(used: u32, context_window: u32) -> u32 {
    used.saturating_mul(100)
        .saturating_add(context_window.saturating_sub(1))
        / context_window
}

fn format_tokens(value: u32) -> String {
    if value >= 1_000_000 {
        format!("{:.1}M", value as f64 / 1_000_000.0)
    } else if value >= 1_000 {
        format!("{:.1}K", value as f64 / 1_000.0)
    } else {
        value.to_string()
    }
}

fn format_duration(duration_ms: Option<u64>) -> String {
    match duration_ms {
        Some(duration_ms) if duration_ms >= 1_000 => {
            format!("{:.1}s", duration_ms as f64 / 1_000.0)
        }
        Some(duration_ms) => format!("{duration_ms}ms"),
        None => "--".to_string(),
    }
}

fn current_account() -> String {
    std::env::var("USER").unwrap_or_else(|_| "local-user".to_string())
}

fn status_account(auth: &AuthSessionReadResponse) -> Option<String> {
    let user = &auth.session.as_ref()?.user;
    user.email
        .as_ref()
        .filter(|email| !email.trim().is_empty())
        .cloned()
        .or_else(|| {
            if user.username.trim().is_empty() {
                None
            } else {
                Some(user.username.clone())
            }
        })
        .or_else(|| {
            if user.user_id.trim().is_empty() {
                None
            } else {
                Some(user.user_id.clone())
            }
        })
}

fn format_path(path: &std::path::Path) -> String {
    let raw = path.display().to_string();
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return raw;
    };
    let Ok(stripped) = path.strip_prefix(&home) else {
        return raw;
    };
    if stripped.as_os_str().is_empty() {
        "~".to_string()
    } else {
        format!("~/{}", stripped.display())
    }
}

#[cfg(test)]
#[path = "status_tests.rs"]
mod tests;
