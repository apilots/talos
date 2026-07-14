//! Talos terminal command mode.
//!
//! This module provides non-interactive terminal commands through the same
//! embedded runtime and TUI gateway adapter used by interactive mode.

use clap::Parser;
use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::sync::Arc;

use talos_sdk::{
    AgentEventPayload, ModelRef, ModelSelection, TalosContext, TalosTimelineContent,
    TalosTimelineDelta, TalosTimelineItemKind, TalosTimelinePhase, TimelineEventEnvelope,
};
use talos_sdk_embedded::EmbeddedServerClient;
use talos_tui::{display_tool_name, GatewayClient as TerminalGatewayClient, TerminalTurnRequest};
use tokio::sync::{mpsc, Mutex};
use tracing::error;

mod types;

use types::{Cli, Commands, ExecCmd, ModelsCmd, SessionCommands};

pub(crate) enum TerminalMode {
    Tui,
    Command(Commands),
}

pub(crate) fn parse_terminal_mode() -> TerminalMode {
    let cli = Cli::parse();
    match cli.command {
        None | Some(Commands::Tui) => TerminalMode::Tui,
        Some(command) => TerminalMode::Command(command),
    }
}

pub(crate) async fn run_cli_with_client(
    command: Commands,
    client: EmbeddedServerClient,
) -> Result<(), Box<dyn std::error::Error>> {
    let client: Arc<dyn TerminalGatewayClient> = Arc::new(Mutex::new(client));
    let result = match command {
        Commands::Exec(cmd) => execute_exec_command(client.clone(), cmd).await,
        Commands::Models(cmd) => execute_models_command(client.clone(), cmd).await,
        Commands::Session(cmd) => execute_session_command(client.clone(), cmd).await,
        Commands::Status => execute_status_command(client.clone()).await,
        Commands::Tui => unreachable!(),
    };
    let shutdown_result = client.shutdown().await;
    result?;
    shutdown_result?;
    Ok(())
}

async fn execute_exec_command(
    client: Arc<dyn TerminalGatewayClient>,
    cmd: ExecCmd,
) -> Result<(), Box<dyn std::error::Error>> {
    let message = cmd.message.join(" ");
    let session_id = cmd
        .session
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let model_selection = if let Some(model) = cmd.model.as_deref() {
        Some(resolve_cli_model_selection(client.as_ref(), model).await?)
    } else {
        None
    };
    let request = TerminalTurnRequest {
        session_id,
        message,
        skills: Vec::new(),
        context: Some(default_terminal_context(
            cmd.user.as_deref().unwrap_or("cli-user"),
        )),
        model_selection,
        current_directory: current_terminal_directory(),
        client_request_id: Some(format!("cli-{}", uuid::Uuid::new_v4().simple())),
    }
    .build();
    let events = client.start_turn_stream(request).await?;
    render_timeline_events(events).await?;
    Ok(())
}

async fn resolve_cli_model_selection(
    client: &dyn TerminalGatewayClient,
    model_key: &str,
) -> Result<ModelSelection, Box<dyn std::error::Error>> {
    let models = client.list_models().await?.models;
    let model = models
        .iter()
        .find(|model| format!("{}:{}", model.provider, model.model.id) == model_key)
        .ok_or_else(|| {
            format!("model '{model_key}' is not available in the current model catalog")
        })?;
    Ok(ModelSelection {
        model_ref: ModelRef {
            provider: model.provider.clone(),
            id: model.model.id.clone(),
        },
        reasoning_effort: model
            .model
            .reasoning
            .then_some(model.model.reasoning_effort),
    })
}

async fn execute_models_command(
    client: Arc<dyn TerminalGatewayClient>,
    cmd: ModelsCmd,
) -> Result<(), Box<dyn std::error::Error>> {
    let models = client.list_models().await?.models;
    if cmd.format == "json" {
        println!("{}", serde_json::to_string_pretty(&models)?);
    } else {
        println!("Available models:");
        for (i, model) in models.iter().enumerate() {
            println!("{}. {} ({})", i, model.model.id, model.provider);
        }
    }
    Ok(())
}

async fn execute_session_command(
    client: Arc<dyn TerminalGatewayClient>,
    cmd: SessionCommands,
) -> Result<(), Box<dyn std::error::Error>> {
    match cmd {
        SessionCommands::List { limit } => {
            let sessions = client.list_sessions().await?;
            println!("Active Sessions:");
            for (i, session) in sessions.iter().take(limit).enumerate() {
                println!(
                    "{}. ID: {} | Model: {}:{} | Messages: {}",
                    i,
                    session.id,
                    session.model_selection.model_ref.provider,
                    session.model_selection.model_ref.id,
                    session.message_count
                );
            }
        }
        SessionCommands::Info { session_id } => {
            let session = client.get_session(&session_id).await?;
            println!("Session Info:");
            println!("  ID: {}", session.id);
            println!(
                "  Model: {}:{}",
                session.model_selection.model_ref.provider, session.model_selection.model_ref.id
            );
            println!("  Messages: {}", session.message_count);
        }
        SessionCommands::SetModel { session_id, model } => {
            let model_selection = resolve_cli_model_selection(client.as_ref(), &model).await?;
            let session = client
                .update_session_model_selection(&session_id, &model_selection)
                .await?;
            println!("Session model updated:");
            println!("  ID: {}", session.id);
            println!(
                "  Model: {}:{}",
                session.model_selection.model_ref.provider, session.model_selection.model_ref.id
            );
        }
    }
    Ok(())
}

async fn execute_status_command(
    client: Arc<dyn TerminalGatewayClient>,
) -> Result<(), Box<dyn std::error::Error>> {
    let sessions = client.list_sessions().await?;
    let models = client.list_models().await?.models;
    println!("Status: Talos runtime accessible");
    println!("Active sessions: {}", sessions.len());
    println!("Available models: {}", models.len());
    Ok(())
}

async fn render_timeline_events(
    mut events: mpsc::Receiver<TimelineEventEnvelope>,
) -> Result<String, Box<dyn std::error::Error>> {
    let mut output_text = String::new();
    let mut output_stream = std::io::stdout();
    let mut item_phases = HashMap::new();
    let mut item_texts = HashMap::new();
    let mut written_final_items = HashSet::new();

    while let Some(envelope) = events.recv().await {
        if render_event(
            &envelope,
            &mut item_phases,
            &mut item_texts,
            &mut written_final_items,
            &mut output_text,
            &mut output_stream,
        )? {
            break;
        }
    }

    if !output_text.is_empty() && !output_text.ends_with('\n') {
        println!();
    }

    Ok(output_text)
}

fn render_event(
    envelope: &TimelineEventEnvelope,
    item_phases: &mut HashMap<String, TalosTimelinePhase>,
    item_texts: &mut HashMap<String, String>,
    written_final_items: &mut HashSet<String>,
    output_text: &mut String,
    output_stream: &mut std::io::Stdout,
) -> Result<bool, Box<dyn std::error::Error>> {
    let event = &envelope.event;
    match &event.payload {
        AgentEventPayload::ItemStarted(payload) => {
            item_phases.insert(payload.item.item_id.clone(), payload.item.phase);
            if let TalosTimelineContent::Text { text } = &payload.item.content {
                item_texts.insert(payload.item.item_id.clone(), text.clone());
            }
            render_started_item(&payload.item);
        }
        AgentEventPayload::ItemDelta(payload) => match &payload.delta {
            TalosTimelineDelta::Text { text } => {
                item_texts
                    .entry(payload.item_id.clone())
                    .or_default()
                    .push_str(text);
                if item_phases.get(&payload.item_id) == Some(&TalosTimelinePhase::FinalAnswer) {
                    output_text.push_str(text);
                    written_final_items.insert(payload.item_id.clone());
                    write!(output_stream, "{}", text)?;
                    output_stream.flush()?;
                } else if !text.trim().is_empty() {
                    eprintln!("TIMELINE_TEXT item={} {}", payload.item_id, text);
                }
            }
            TalosTimelineDelta::ToolArguments { arguments_delta } => {
                eprintln!(
                    "TOOL_CALL_ARGS_DELTA item={} delta={}",
                    payload.item_id, arguments_delta
                );
            }
            TalosTimelineDelta::ToolProgress {
                progress_kind,
                text_delta,
            } => {
                eprintln!(
                    "TOOL_CALL_PROGRESS item={} kind={:?} message={}",
                    payload.item_id, progress_kind, text_delta
                );
            }
            TalosTimelineDelta::ToolResult { is_error, .. } => {
                eprintln!(
                    "TOOL_CALL_RESULT item={} is_error={}",
                    payload.item_id, is_error
                );
            }
            TalosTimelineDelta::ToolDynamicRequest {
                call_id, tool_name, ..
            } => {
                eprintln!(
                    "TOOL_CALL_DYNAMIC tool={} internal={} call_id={}",
                    display_tool_name(tool_name),
                    tool_name,
                    call_id
                );
            }
            TalosTimelineDelta::ContextCompaction {
                stage,
                outcome,
                message,
                ..
            } => {
                eprintln!(
                    "CONTEXT_COMPACTION item={} stage={:?} outcome={:?} message={}",
                    payload.item_id,
                    stage,
                    outcome,
                    message.as_deref().unwrap_or("")
                );
            }
            TalosTimelineDelta::Status { status } => {
                eprintln!("ITEM_STATUS item={} status={:?}", payload.item_id, status);
            }
        },
        AgentEventPayload::AssistantTextCommitted(payload) => {
            item_phases.insert(payload.item_id.clone(), payload.phase);
            if payload.phase == TalosTimelinePhase::FinalAnswer
                && !written_final_items.contains(&payload.item_id)
            {
                if let Some(text) = item_texts.get(&payload.item_id) {
                    written_final_items.insert(payload.item_id.clone());
                    output_text.push_str(text);
                    write!(output_stream, "{}", text)?;
                    output_stream.flush()?;
                }
            }
        }
        AgentEventPayload::ItemCompleted(payload) => {
            if let Some(TalosTimelineContent::ToolCall {
                call_id,
                name,
                result,
                ..
            }) = &payload.final_content
            {
                eprintln!(
                    "TOOL_CALL_COMPLETED call_id={} tool={} internal={} is_error={}",
                    call_id,
                    display_tool_name(name),
                    name,
                    result.as_ref().map(|value| value.is_error).unwrap_or(false)
                );
            }
            if item_phases.get(&payload.item_id) == Some(&TalosTimelinePhase::FinalAnswer) {
                if let Some(TalosTimelineContent::Text { text }) = &payload.final_content {
                    if !written_final_items.contains(&payload.item_id) {
                        written_final_items.insert(payload.item_id.clone());
                        output_text.push_str(text);
                        write!(output_stream, "{}", text)?;
                        output_stream.flush()?;
                    }
                }
            }
        }
        AgentEventPayload::TurnError(payload) => {
            error!("TURN_ERROR {}", payload.error);
            return Err(payload.error.clone().into());
        }
        AgentEventPayload::TurnCompleted(_) => {
            return Ok(true);
        }
        AgentEventPayload::TurnMetrics(_) => {}
        AgentEventPayload::TurnStarted(_) => {}
    }
    Ok(false)
}

fn render_started_item(item: &talos_sdk::TalosTimelineItem) {
    match (&item.kind, &item.phase, &item.content) {
        (TalosTimelineItemKind::Reasoning, TalosTimelinePhase::Thinking, _) => {
            eprintln!("THINKING item={}", item.item_id);
        }
        (
            TalosTimelineItemKind::ExecutionSummary,
            _,
            TalosTimelineContent::ExecutionSummary { summary },
        ) => {
            if !summary.trim().is_empty() {
                eprintln!("SUMMARY {}", summary);
            }
        }
        (
            TalosTimelineItemKind::ContextCompaction,
            _,
            TalosTimelineContent::ContextCompaction { stage, message, .. },
        ) => {
            eprintln!(
                "CONTEXT_COMPACTION_STARTED item={} stage={:?} message={}",
                item.item_id,
                stage,
                message.as_deref().unwrap_or("")
            );
        }
        (
            TalosTimelineItemKind::ToolCall,
            _,
            TalosTimelineContent::ToolCall { call_id, name, .. },
        ) => {
            eprintln!(
                "TOOL_CALL_STARTED name={} internal={} call_id={} item={}",
                display_tool_name(name),
                name,
                call_id,
                item.item_id
            );
        }
        _ => {}
    }
}

fn default_terminal_context(user_id: &str) -> TalosContext {
    TalosContext::cli(user_id.to_string()).with_channel_id("cli")
}

fn current_terminal_directory() -> Option<std::path::PathBuf> {
    std::env::current_dir().ok()
}

#[cfg(test)]
#[path = "cli_tests.rs"]
mod tests;
