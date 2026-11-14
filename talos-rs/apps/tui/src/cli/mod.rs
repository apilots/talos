//! Talos terminal command mode.
//!
//! This module provides non-interactive terminal commands through the same
//! embedded runtime and TUI gateway adapter used by interactive mode.

use clap::Parser;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::Write;
use std::sync::Arc;

use talos_sdk::{
    AgentAssistantTextPhase, AgentEventPayload, AgentMessageBlock, AgentMessageBlockContent,
    AgentMessageBlockDeltaContent, AgentPendingInput, AgentToolCallArguments, ErrorCode,
    ModelSelection, TimelineEvent, UserActionParams, UserPermissionMode,
};
use talos_sdk_embedded::EmbeddedServerClient;
use talos_tui::{
    display_tool_name, selection_for_key, GatewayClient as TerminalGatewayClient,
    TerminalTurnRequest,
};
use tokio::sync::{mpsc, Mutex};

mod automation;
mod plugin;
mod trajectory;
mod types;

use trajectory::ExecObservation;
use types::{Cli, Commands, ExecCmd, ModelsCmd, SessionCommands};

pub(crate) enum TerminalMode {
    Tui,
    Command(Commands),
}

pub(crate) fn session_ownership(mode: &TerminalMode) -> talos_base::SessionOwnershipPolicy {
    let (kind, hold, run_session_loops) = match mode {
        TerminalMode::Tui => (
            talos_error::SessionOwnerKind::Tui,
            talos_base::SessionHold::Attached,
            true,
        ),
        TerminalMode::Command(Commands::Exec(_)) => (
            talos_error::SessionOwnerKind::Exec,
            talos_base::SessionHold::Process,
            false,
        ),
        TerminalMode::Command(Commands::Goal(command))
            if matches!(command.command, Some(types::GoalCommands::Start { .. })) =>
        {
            (
                talos_error::SessionOwnerKind::Cli,
                talos_base::SessionHold::Process,
                true,
            )
        }
        TerminalMode::Command(Commands::Loop(command))
            if matches!(command.command, Some(types::LoopCommands::Start { .. })) =>
        {
            (
                talos_error::SessionOwnerKind::Cli,
                talos_base::SessionHold::Process,
                true,
            )
        }
        TerminalMode::Command(_) => (
            talos_error::SessionOwnerKind::Cli,
            talos_base::SessionHold::Turn,
            false,
        ),
    };
    talos_base::SessionOwnershipPolicy {
        kind,
        hold,
        run_session_loops,
    }
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
    mut client: EmbeddedServerClient,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Commands::Plugin(command) = command {
        let result = plugin::execute(&mut client, command).await;
        let shutdown_result = talos_sdk::GatewayClient::shutdown(&mut client).await;
        let response = result?;
        shutdown_result?;
        println!("{}", serde_json::to_string_pretty(&response)?);
        return Ok(());
    }
    let client = Arc::new(Mutex::new(client));
    let result = match command {
        Commands::Exec(cmd) => execute_exec_command(client.clone(), cmd).await,
        Commands::Models(cmd) => execute_models_command(client.clone(), cmd).await,
        Commands::Session(cmd) => execute_session_command(client.clone(), cmd).await,
        Commands::Status => execute_status_command(client.clone()).await,
        Commands::Goal(cmd) => {
            run_foreground_result(client.clone(), automation::goal(client.clone(), cmd).await).await
        }
        Commands::Loop(cmd) => {
            run_foreground_result(
                client.clone(),
                automation::loop_task(client.clone(), cmd).await,
            )
            .await
        }
        Commands::Automation(cmd) => {
            print_automation_result(automation::management(client.clone(), cmd).await)
        }
        Commands::Tui | Commands::Plugin(_) => unreachable!(),
    };
    let shutdown_result = client.shutdown().await;
    result?;
    shutdown_result?;
    Ok(())
}

async fn run_foreground_result(
    client: Arc<dyn TerminalGatewayClient>,
    result: talos_error::TalosResult<automation::CommandOutput>,
) -> Result<(), Box<dyn std::error::Error>> {
    let output = result?;
    println!("{}", serde_json::to_string_pretty(&output.value)?);
    if let Some((session, object)) = output.foreground {
        eprintln!("Keeping this embedded automation owner in the foreground. Keep this process running; Ctrl-C retains the existing process termination behavior.");
        talos_tui::wait_for_session_automation(client, session, object).await?;
    }
    Ok(())
}

fn print_automation_result(
    result: talos_error::TalosResult<serde_json::Value>,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("{}", serde_json::to_string_pretty(&result?)?);
    Ok(())
}

async fn execute_exec_command(
    client: Arc<Mutex<EmbeddedServerClient>>,
    cmd: ExecCmd,
) -> Result<(), Box<dyn std::error::Error>> {
    let trace_file = cmd.trajectory.as_ref().map(File::create_new).transpose()?;
    let transcript_file = cmd.transcript.as_ref().map(File::create_new).transpose()?;
    let mut observation = ExecObservation::new(transcript_file.is_some());
    let result = execute_turn(client.clone(), cmd, &mut observation).await;
    eprintln!(
        "TALOS_EXEC_METRICS {}",
        serde_json::to_string(&observation.metrics)?
    );
    if let Some(file) = trace_file {
        let export = observation
            .write_exports(&mut *client.lock().await, file, transcript_file)
            .await;
        if let Err(error) = export {
            eprintln!("Unable to export execution trajectory: {error}");
            result?;
            return Err(error);
        }
    }
    result
}

async fn execute_turn(
    client: Arc<dyn TerminalGatewayClient>,
    cmd: ExecCmd,
    observation: &mut ExecObservation,
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
    let mut request = TerminalTurnRequest {
        session_id,
        message,
        skills: Vec::new(),
        file_ids: Vec::new(),
        model_selection,
        current_directory: current_terminal_directory(),
        project_id: None,
        client_request_id: Some(match cmd.request_id.as_deref() {
            Some(request_id) if !request_id.trim().is_empty() => request_id.to_owned(),
            Some(_) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "request id must not be empty",
                )
                .into());
            }
            None => format!("cli-{}", uuid::Uuid::new_v4().simple()),
        }),
    }
    .build();
    if cmd.dangerously_skip_permissions {
        eprintln!(
            "WARNING: Saving Full Access for this session. Use only in disposable environments."
        );
        match client.get_session(&request.session_id).await {
            Ok(_) => {
                client
                    .update_session_permission_mode(
                        &request.session_id,
                        UserPermissionMode::FullAccess,
                    )
                    .await?;
            }
            Err(error) if error.is_code(ErrorCode::ResourceNotFound) => {
                request.user_permission_mode = Some(UserPermissionMode::FullAccess);
            }
            Err(error) => return Err(exec_failure(error)),
        }
    }
    let events = match client
        .start_turn_stream(request, /*accepted_turn*/ None)
        .await
    {
        Ok(events) => events,
        Err(error) => return Err(exec_failure(error)),
    };
    render_timeline_events(events, observation).await
}

async fn resolve_cli_model_selection(
    client: &dyn TerminalGatewayClient,
    model_key: &str,
) -> Result<ModelSelection, Box<dyn std::error::Error>> {
    let models = client.list_models().await?.models;
    selection_for_key(&models, model_key, /*reasoning_effort*/ None).ok_or_else(|| {
        format!("model '{model_key}' is not available in the current model catalog").into()
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
    mut events: mpsc::Receiver<TimelineEvent>,
    observation: &mut ExecObservation,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut output_text = String::new();
    let mut output_stream = std::io::stdout();
    let mut item_phases = HashMap::new();
    let mut item_texts = HashMap::new();
    let mut written_final_items = HashSet::new();
    let mut turn_identity = None;
    let terminal_status;

    let result = loop {
        let Some(envelope) = events.recv().await else {
            terminal_status = Some("failed");
            break Err("Turn event stream closed before a terminal event.".into());
        };
        if let talos_sdk::TimelineEventPayload::Agent(event) = &envelope.event {
            turn_identity = Some((
                event.identity.session_id.clone(),
                event.identity.turn_id.clone(),
            ));
            observation.record(event);
        }
        match render_event(
            &envelope,
            &mut item_phases,
            &mut item_texts,
            &mut written_final_items,
            &mut output_text,
            &mut output_stream,
        ) {
            Ok(false) => {}
            Ok(true) => {
                terminal_status = Some("completed");
                break Ok(());
            }
            Err(error) if error.is::<ExecWaitingForInput>() => {
                terminal_status = Some("waiting");
                break Err(error);
            }
            Err(error) => {
                terminal_status = Some("failed");
                break Err(error);
            }
        }
    };

    if !output_text.is_empty() && !output_text.ends_with('\n') {
        println!();
    }
    if let (Some(status), Some((session_id, turn_id))) = (terminal_status, turn_identity.as_ref()) {
        print_exec_result(session_id, turn_id, status);
    }

    result
}

#[derive(Debug)]
pub(crate) struct ExecWaitingForInput;

impl ExecWaitingForInput {
    pub(crate) const EXIT_CODE: u8 = 3;
}

impl std::fmt::Display for ExecWaitingForInput {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Turn is waiting for input.")
    }
}

impl std::error::Error for ExecWaitingForInput {}

#[derive(Debug)]
pub(crate) struct ExecSessionHeld {
    message: String,
}

impl ExecSessionHeld {
    pub(crate) const EXIT_CODE: u8 = 4;
}

impl std::fmt::Display for ExecSessionHeld {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ExecSessionHeld {}

pub(crate) fn public_error_message(error: &(dyn std::error::Error + 'static)) -> String {
    if let Some(error) = error.downcast_ref::<talos_error::TalosError>() {
        return error.to_api_error().message;
    }
    if let Some(held) = error.downcast_ref::<ExecSessionHeld>() {
        return held.message.clone();
    }
    error.to_string()
}

pub(crate) fn is_session_held(error: &(dyn std::error::Error + 'static)) -> bool {
    error.is::<ExecSessionHeld>()
        || error
            .downcast_ref::<talos_error::TalosError>()
            .is_some_and(|error| error.is_code(ErrorCode::SessionHeld))
}

fn exec_failure(error: talos_error::TalosError) -> Box<dyn std::error::Error> {
    let message = error.to_api_error().message;
    if error.is_code(ErrorCode::SessionHeld) {
        ExecSessionHeld { message }.into()
    } else {
        message.into()
    }
}

fn waiting_input_line(session_id: &str, turn_id: &str, pending: &AgentPendingInput) -> String {
    let request = serde_json::to_string(pending).unwrap_or_else(|_| "{}".to_owned());
    match pending {
        AgentPendingInput::Message { action_id, .. } => format!(
            "WAITING_FOR_INPUT session={session_id} turn={turn_id} kind=message action_id={action_id} request={request}"
        ),
        AgentPendingInput::UserAction { action } => {
            let (kind, tool_name) = match &action.params {
                UserActionParams::SandboxApproval(params) => {
                    ("approval", params.tool_name.as_str())
                }
                UserActionParams::Question(_) => ("question", "AskUserQuestion"),
                UserActionParams::DynamicToolResult(params) => {
                    ("user_action", params.tool_name.as_str())
                }
            };
            format!(
                "WAITING_FOR_INPUT session={session_id} turn={turn_id} kind={kind} tool={tool_name} call_id={} action_id={} request={request}",
                action.call_id, action.action_id,
            )
        }
    }
}

fn report_waiting_input(session_id: &str, turn_id: &str, pending: &AgentPendingInput) {
    eprintln!("{}", waiting_input_line(session_id, turn_id, pending));
}

fn exec_result_line(session_id: &str, turn_id: &str, status: &str) -> String {
    serde_json::json!({
        "sessionId": session_id,
        "turnId": turn_id,
        "status": status,
    })
    .to_string()
}

fn print_exec_result(session_id: &str, turn_id: &str, status: &str) {
    println!("{}", exec_result_line(session_id, turn_id, status));
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
                    "TOOL_CALL_COMPLETED call_id={} tool={} internal={} is_error={} metrics={}",
                    result.call_id,
                    display_tool_name(&result.tool_name),
                    result.tool_name,
                    result.is_error,
                    serde_json::to_string(&result.metrics)?
                );
            }
            if let AgentMessageBlockContent::ContextCompaction(compaction) = &payload.block.content
            {
                eprintln!(
                    "CONTEXT_COMPACTION_COMPLETED block={} metrics={}",
                    payload.block.block_id,
                    serde_json::to_string(&compaction.metrics)?
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
            return Err(payload.error.message.clone().into());
        }
        AgentEventPayload::TurnCompleted(_) => {
            return Ok(true);
        }
        AgentEventPayload::WaitingForInput(pending) => {
            report_waiting_input(&event.identity.session_id, &event.identity.turn_id, pending);
            return Err(ExecWaitingForInput.into());
        }
        AgentEventPayload::TurnStarted(_)
        | AgentEventPayload::MessageStarted(_)
        | AgentEventPayload::MessageCompleted(_)
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
