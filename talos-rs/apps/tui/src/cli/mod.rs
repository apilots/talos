//! Talos terminal command mode.
//!
//! This module provides non-interactive terminal commands through the same
//! embedded runtime and TUI gateway adapter used by interactive mode.

use clap::Parser;
use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::sync::Arc;

use talos_sdk::{
    AgentAssistantTextPhase, AgentEventPayload, AgentMessageBlock, AgentMessageBlockContent,
    AgentMessageBlockDeltaContent, AgentToolCallArguments, ModelRef, ModelSelection, TimelineEvent,
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
        file_ids: Vec::new(),
        model_selection,
        current_directory: current_terminal_directory(),
        project_id: None,
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
    Ok(ModelSelection::from_model(
        ModelRef {
            provider: model.provider.clone(),
            id: model.model.id.clone(),
        },
        &model.model,
    ))
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
    mut events: mpsc::Receiver<TimelineEvent>,
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
    envelope: &TimelineEvent,
    item_phases: &mut HashMap<String, AgentAssistantTextPhase>,
    item_texts: &mut HashMap<String, String>,
    written_final_items: &mut HashSet<String>,
    output_text: &mut String,
    output_stream: &mut std::io::Stdout,
) -> Result<bool, Box<dyn std::error::Error>> {
    let event = match &envelope.event {
        talos_sdk::TimelineEventPayload::Agent(event) => event,
        talos_sdk::TimelineEventPayload::SystemNotice(notice) => {
            eprintln!(
                "NOTICE {:?}: {}",
                notice.notice.level,
                notice.notice.message()
            );
            return Ok(false);
        }
    };
    match &event.payload {
        AgentEventPayload::MessageBlockStarted(payload) => {
            if let AgentMessageBlockContent::AssistantText(assistant) = &payload.block.content {
                item_phases.insert(payload.block.block_id.clone(), assistant.phase);
                item_texts.insert(payload.block.block_id.clone(), assistant.text.clone());
            }
            render_started_block(&payload.block);
        }
        AgentEventPayload::MessageBlockDelta(payload) => match &payload.delta {
            AgentMessageBlockDeltaContent::Text(text) => {
                item_texts
                    .entry(payload.block_id.clone())
                    .or_default()
                    .push_str(text);
                if item_phases.get(&payload.block_id) == Some(&AgentAssistantTextPhase::FinalAnswer)
                {
                    output_text.push_str(text);
                    written_final_items.insert(payload.block_id.clone());
                    write!(output_stream, "{}", text)?;
                    output_stream.flush()?;
                } else if !text.trim().is_empty() {
                    eprintln!("TIMELINE_TEXT block={} {}", payload.block_id, text);
                }
            }
            AgentMessageBlockDeltaContent::AssistantTextPhase(phase) => {
                item_phases.insert(payload.block_id.clone(), *phase);
                if *phase == AgentAssistantTextPhase::FinalAnswer
                    && !written_final_items.contains(&payload.block_id)
                {
                    if let Some(text) = item_texts.get(&payload.block_id) {
                        written_final_items.insert(payload.block_id.clone());
                        output_text.push_str(text);
                        write!(output_stream, "{}", text)?;
                        output_stream.flush()?;
                    }
                }
            }
            AgentMessageBlockDeltaContent::ToolCallArguments(arguments) => {
                eprintln!(
                    "TOOL_CALL_ARGS_DELTA block={} delta={}",
                    payload.block_id, arguments.json_fragment
                );
            }
            AgentMessageBlockDeltaContent::ToolProgress(progress) => {
                eprintln!(
                    "TOOL_CALL_PROGRESS block={} kind={:?} message={}",
                    payload.block_id, progress.kind, progress.text
                );
            }
            AgentMessageBlockDeltaContent::ContextCompaction(delta) => {
                eprintln!(
                    "CONTEXT_COMPACTION block={} stage={:?} outcome={:?} message={}",
                    payload.block_id,
                    delta.stage,
                    delta.outcome,
                    delta.message.as_deref().unwrap_or("")
                );
            }
        },
        AgentEventPayload::MessageBlockCompleted(payload) => {
            if let AgentMessageBlockContent::ToolResult(result) = &payload.block.content {
                eprintln!(
                    "TOOL_CALL_COMPLETED call_id={} tool={} internal={} is_error={}",
                    result.call_id,
                    display_tool_name(&result.tool_name),
                    result.tool_name,
                    result.is_error
                );
            }
            if let AgentMessageBlockContent::AssistantText(assistant) = &payload.block.content {
                if assistant.phase == AgentAssistantTextPhase::FinalAnswer
                    && !written_final_items.contains(&payload.block.block_id)
                {
                    written_final_items.insert(payload.block.block_id.clone());
                    let text = &assistant.text;
                    output_text.push_str(text);
                    write!(output_stream, "{}", text)?;
                    output_stream.flush()?;
                }
            }
        }
        AgentEventPayload::TurnFailed(payload) => {
            error!("TURN_ERROR {}", payload.error.message);
            return Err(payload.error.message.clone().into());
        }
        AgentEventPayload::TurnCompleted(_) => {
            return Ok(true);
        }
        AgentEventPayload::TurnStarted(_)
        | AgentEventPayload::MessageStarted(_)
        | AgentEventPayload::MessageCompleted(_)
        | AgentEventPayload::WaitingForInput(_)
        | AgentEventPayload::MetricsUpdated(_) => {}
    }
    Ok(false)
}

fn render_started_block(block: &AgentMessageBlock) {
    match &block.content {
        AgentMessageBlockContent::Reasoning(_) => {
            eprintln!("THINKING block={}", block.block_id);
        }
        AgentMessageBlockContent::ExecutionSummary(summary) => {
            if !summary.text.trim().is_empty() {
                eprintln!("SUMMARY {}", summary.text);
            }
        }
        AgentMessageBlockContent::ContextCompaction(compaction) => {
            eprintln!(
                "CONTEXT_COMPACTION_STARTED block={} stage={:?} message={}",
                block.block_id,
                compaction.stage,
                compaction.message.as_deref().unwrap_or("")
            );
        }
        AgentMessageBlockContent::ToolCall(tool) => {
            let arguments = match &tool.arguments {
                AgentToolCallArguments::Streaming { json_fragment } => json_fragment.as_str(),
                AgentToolCallArguments::Complete { .. } => "<complete>",
            };
            eprintln!(
                "TOOL_CALL_STARTED name={} internal={} call_id={} block={} arguments={}",
                display_tool_name(&tool.name),
                tool.name,
                tool.call_id,
                block.block_id,
                arguments
            );
        }
        _ => {}
    }
}

fn current_terminal_directory() -> Option<std::path::PathBuf> {
    std::env::current_dir().ok()
}

#[cfg(test)]
#[path = "cli_tests.rs"]
mod tests;
