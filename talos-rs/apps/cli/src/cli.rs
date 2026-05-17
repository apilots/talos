//! Talos Command Line Interface
//!
//! This module provides a command-line interface for interacting with the Talos system
//! through the local JSON-RPC app-server client.

use clap::Parser;
use std::collections::HashMap;
use std::io::Write;
use talos_client::{
    AppServerClient, AppServerClientConfig, EventSubscribeParams, ProtocolTalosContext,
    SessionCreateParams, TalosInputItem, TalosMessageRequest, TalosTimelineContent,
    TalosTimelineDelta, TalosTimelineEventPayload, TalosTimelineItemKind, TalosTimelinePhase,
    TimelineEventEnvelope,
};
use tracing::{debug, error};

mod types;

use types::{AppServerSmokeCmd, ChatCommands, Cli, Commands, ModelsCmd, SessionCommands};

/// Main function to run the CLI
pub async fn run_cli() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    run_app_server_cli(cli).await
}

async fn run_app_server_cli(cli: Cli) -> Result<(), Box<dyn std::error::Error>> {
    let app_server_bin = cli
        .app_server_bin
        .or_else(|| std::env::var("TALOS_APP_SERVER_BIN").ok())
        .unwrap_or_else(|| "talos".to_string());
    let launch_token = cli
        .launch_token
        .or_else(|| std::env::var("TALOS_LOCAL_LAUNCH_TOKEN").ok())
        .unwrap_or_else(|| format!("cli-{}", uuid::Uuid::new_v4().simple()));
    let mut client = AppServerClient::connect(AppServerClientConfig {
        app_server_bin,
        launch_token,
        client_name: "talos-cli".to_string(),
        client_version: env!("CARGO_PKG_VERSION").to_string(),
    })
    .await?;

    let result = match cli.command {
        Commands::Chat(cmd) => execute_app_server_chat_command(&mut client, cmd).await,
        Commands::Session(cmd) => execute_app_server_session_command(&mut client, cmd).await,
        Commands::Models(cmd) => execute_app_server_models_command(&mut client, cmd).await,
        Commands::Status => execute_app_server_status_command(&mut client).await,
        Commands::Info => execute_app_server_info_command().await,
        Commands::AppServerSmoke(cmd) => {
            execute_app_server_smoke_with_client(&mut client, cmd).await
        }
    };
    let shutdown_result = client.shutdown().await;
    result?;
    shutdown_result?;
    Ok(())
}

async fn execute_app_server_smoke_with_client(
    client: &mut AppServerClient,
    cmd: AppServerSmokeCmd,
) -> Result<(), Box<dyn std::error::Error>> {
    match cmd.scenario.as_str() {
        "smoke" => run_app_server_smoke(client).await?,
        "turn-smoke" => run_app_server_turn_smoke(client).await?,
        "approval-resume" => run_app_server_approval_resume(client).await?,
        other => return Err(format!("unsupported app-server smoke scenario: {other}").into()),
    }
    println!("App-server scenario passed: {}", cmd.scenario);
    Ok(())
}

async fn run_app_server_smoke(
    client: &mut AppServerClient,
) -> Result<(), Box<dyn std::error::Error>> {
    client.list_models().await?;
    client.config_read().await?;
    let session = client
        .create_session(SessionCreateParams {
            context: default_chat_context("cli-app-server"),
            model: None,
            project_id: None,
            current_directory: current_cli_directory(),
        })
        .await?;
    client
        .subscribe_events(EventSubscribeParams {
            session_id: session.context.session_id,
            since_event_id: None,
            replay_limit: Some(32),
            close_when_idle: None,
            close_on_terminal: None,
        })
        .await?;
    Ok(())
}

async fn run_app_server_turn_smoke(
    client: &mut AppServerClient,
) -> Result<(), Box<dyn std::error::Error>> {
    let session = client
        .create_session(SessionCreateParams {
            context: default_chat_context("cli-app-server"),
            model: None,
            project_id: None,
            current_directory: current_cli_directory(),
        })
        .await?;
    let session_id = session.context.session_id;
    client
        .subscribe_events(EventSubscribeParams {
            session_id: session_id.clone(),
            since_event_id: None,
            replay_limit: Some(32),
            close_when_idle: None,
            close_on_terminal: None,
        })
        .await?;
    client
        .start_turn(
            &session_id,
            TalosMessageRequest {
                input: vec![TalosInputItem::Text {
                    text: "answer with ok".to_string(),
                }],
                workspace_id: None,
            },
            Some(format!("cli-app-server-{}", uuid::Uuid::new_v4().simple())),
        )
        .await?;

    let mut saw_started = false;
    let mut saw_delta = false;
    for _ in 0..128 {
        let envelope = client.next_event().await?;
        if envelope.session_id != session_id {
            continue;
        }
        match envelope.event.payload {
            TalosTimelineEventPayload::TurnStarted(_) => saw_started = true,
            TalosTimelineEventPayload::ItemDelta(payload) => {
                if matches!(payload.delta, TalosTimelineDelta::Text { .. }) {
                    saw_delta = true;
                }
            }
            TalosTimelineEventPayload::TurnCompleted(_) if saw_started && saw_delta => {
                return Ok(());
            }
            TalosTimelineEventPayload::TurnError(payload) => {
                return Err(format!("app-server turn failed: {}", payload.error).into());
            }
            _ => {}
        }
    }
    Err("app-server turn smoke did not receive a complete event flow".into())
}

async fn run_app_server_approval_resume(
    client: &mut AppServerClient,
) -> Result<(), Box<dyn std::error::Error>> {
    let session = client
        .create_session(SessionCreateParams {
            context: default_chat_context("cli-app-server-approval"),
            model: None,
            project_id: None,
            current_directory: current_cli_directory(),
        })
        .await?;
    let session_id = session.context.session_id;
    let setup_subscription = client
        .subscribe_events(EventSubscribeParams {
            session_id: session_id.clone(),
            since_event_id: None,
            replay_limit: Some(128),
            close_when_idle: None,
            close_on_terminal: None,
        })
        .await?;

    client
        .start_turn(
            &session_id,
            TalosMessageRequest {
                input: vec![TalosInputItem::Text {
                    text: "Call the Bash tool exactly once with command `echo APPROVAL_RESUME_OK`, timeout 10, and dangerously_disable_sandbox true. Do not answer directly before calling the tool.".to_string(),
                }],
                workspace_id: None,
            },
            Some(format!(
                "cli-app-server-approval-pending-{}",
                uuid::Uuid::new_v4().simple()
            )),
        )
        .await?;

    let mut pending_call_id = None;
    let mut pending_tool_name = None;
    let mut pending_event_id = None;
    for _ in 0..1024 {
        let envelope = client.next_event().await?;
        if envelope.session_id != session_id {
            continue;
        }
        match envelope.event.payload {
            TalosTimelineEventPayload::ItemDelta(payload) => {
                if let TalosTimelineDelta::ToolApprovalRequest {
                    call_id, tool_name, ..
                } = payload.delta
                {
                    pending_call_id = Some(call_id);
                    pending_tool_name = Some(tool_name);
                    pending_event_id = Some(envelope.event_id.clone());
                }
            }
            TalosTimelineEventPayload::TurnCompleted(_) if pending_call_id.is_some() => {
                break;
            }
            TalosTimelineEventPayload::TurnError(payload) => {
                return Err(format!("app-server approval setup failed: {}", payload.error).into());
            }
            _ => {}
        }
    }

    let call_id = pending_call_id.ok_or("missing pending approval call_id")?;
    let tool_name = pending_tool_name.ok_or("missing pending approval tool_name")?;
    let event_id = pending_event_id.ok_or("missing pending approval event_id")?;
    client
        .unsubscribe_events(&setup_subscription.subscription_id)
        .await?;
    client
        .submit_tool_approval(
            &session_id,
            &call_id,
            &tool_name,
            true,
            Some("approved by cli regression".to_string()),
        )
        .await?;
    client
        .subscribe_events(EventSubscribeParams {
            session_id: session_id.clone(),
            since_event_id: Some(event_id),
            replay_limit: Some(512),
            close_when_idle: None,
            close_on_terminal: None,
        })
        .await?;

    let mut saw_tool_completed = false;
    for _ in 0..1024 {
        let envelope = client.next_event().await?;
        if envelope.session_id != session_id {
            continue;
        }
        match envelope.event.payload {
            TalosTimelineEventPayload::ItemDelta(payload) => {
                if matches!(payload.delta, TalosTimelineDelta::ToolResult { .. }) {
                    saw_tool_completed = true;
                }
            }
            TalosTimelineEventPayload::ItemCompleted(payload) => {
                if timeline_tool_call_id(payload.final_content.as_ref()).as_deref()
                    == Some(call_id.as_str())
                {
                    saw_tool_completed = true;
                }
            }
            TalosTimelineEventPayload::TurnCompleted(_) if saw_tool_completed => return Ok(()),
            TalosTimelineEventPayload::TurnError(payload) => {
                return Err(format!("app-server approval resume failed: {}", payload.error).into());
            }
            _ => {}
        }
    }

    Err("app-server approval resume did not complete approved tool call".into())
}

fn timeline_tool_call_id(content: Option<&TalosTimelineContent>) -> Option<String> {
    match content {
        Some(TalosTimelineContent::ToolCall { call_id, .. }) => Some(call_id.clone()),
        _ => None,
    }
}

async fn execute_app_server_chat_command(
    client: &mut AppServerClient,
    cmd: ChatCommands,
) -> Result<(), Box<dyn std::error::Error>> {
    match cmd {
        ChatCommands::Simple {
            message,
            model,
            user,
            stream,
        } => {
            let session = client
                .create_session(SessionCreateParams {
                    context: default_chat_context(user.as_deref().unwrap_or("cli-user")),
                    model: model.clone(),
                    project_id: None,
                    current_directory: current_cli_directory(),
                })
                .await?;
            let request = build_message_request(&message, model, stream);
            send_and_render_app_server_message(client, &session.context.session_id, request)
                .await?;
        }
        ChatCommands::In {
            session_id,
            message,
            model,
            stream,
        } => {
            let request = build_message_request(&message, model, stream);
            send_and_render_app_server_message(client, &session_id, request).await?;
        }
        ChatCommands::Recent {
            index,
            message,
            model,
            stream,
        } => {
            let session_id = client
                .list_sessions(Default::default())
                .await
                .ok()
                .and_then(|sessions| sessions.into_iter().nth(index))
                .map(|session| session.context.session_id);
            if let Some(session_id) = session_id {
                let request = build_message_request(&message, model, stream);
                send_and_render_app_server_message(client, &session_id, request).await?;
            } else {
                debug!("No recent session found at index {}", index);
            }
        }
    }
    Ok(())
}

async fn execute_app_server_session_command(
    client: &mut AppServerClient,
    cmd: SessionCommands,
) -> Result<(), Box<dyn std::error::Error>> {
    match cmd {
        SessionCommands::Create { model, name: _ } => {
            let session = client
                .create_session(SessionCreateParams {
                    context: default_chat_context("cli-user"),
                    model,
                    project_id: None,
                    current_directory: current_cli_directory(),
                })
                .await?;
            println!(
                "Session created: {} (model: {})",
                session.context.session_id, session.config.llm.model
            );
        }
        SessionCommands::List { limit, user: _ } => {
            let sessions = client.list_sessions(Default::default()).await?;
            println!("Active Sessions:");
            for (i, session) in sessions.iter().take(limit).enumerate() {
                println!(
                    "{}. ID: {} | Model: {} | Messages: {}",
                    i,
                    session.context.session_id,
                    session.config.llm.model,
                    session.state.message_count
                );
            }
        }
        SessionCommands::Info { session_id } => {
            let session = client.get_session(&session_id).await?;
            println!("Session Info:");
            println!("  ID: {}", session.context.session_id);
            println!("  Model: {}", session.config.llm.model);
            println!("  Messages: {}", session.state.message_count);
        }
        SessionCommands::SetModel { session_id, model } => {
            let session = client.update_session_model(&session_id, &model).await?;
            println!("Session model updated:");
            println!("  ID: {}", session.context.session_id);
            println!("  Model: {}", session.config.llm.model);
        }
        SessionCommands::Interactive {
            session_id,
            model,
            stream,
        } => {
            println!("Entering interactive mode for session: {}", session_id);
            println!("Exit with 'quit' or 'exit'");
            loop {
                use std::io::{self, BufRead};
                print!("> ");
                io::stdout().flush().unwrap();

                let stdin = io::stdin();
                let mut line = String::new();
                stdin.lock().read_line(&mut line).unwrap();
                let input = line.trim();
                if input == "quit" || input == "exit" {
                    if let Err(err) = client.delete_session(&session_id).await {
                        debug!("Failed to close session {}: {}", session_id, err);
                    }
                    break;
                }
                if !input.is_empty() {
                    let request = build_message_request(input, model.clone(), stream);
                    if let Err(err) =
                        send_and_render_app_server_message(client, &session_id, request).await
                    {
                        debug!("Error: {:?}", err);
                    }
                }
            }
            println!("Exited interactive mode");
        }
        SessionCommands::ApproveTool {
            session_id,
            call_id,
            tool_name,
            approved,
            reason,
        } => {
            client
                .submit_tool_approval(&session_id, &call_id, &tool_name, approved, reason)
                .await?;
            let verdict = if approved { "approved" } else { "rejected" };
            println!(
                "Tool call {} ({}) {}: {}",
                display_tool_name(&tool_name),
                tool_name,
                verdict,
                call_id
            );
        }
    }
    Ok(())
}

async fn execute_app_server_models_command(
    client: &mut AppServerClient,
    cmd: ModelsCmd,
) -> Result<(), Box<dyn std::error::Error>> {
    let models = client.list_models().await?;
    if cmd.format == "json" {
        println!("{}", serde_json::to_string_pretty(&models)?);
    } else {
        println!("Available models:");
        for (i, model) in models.iter().enumerate() {
            println!("{}. {} ({})", i, model.id, model.provider);
        }
    }
    Ok(())
}

async fn execute_app_server_status_command(
    client: &mut AppServerClient,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("Status: App-server accessible");
    println!(
        "Active sessions: {}",
        client
            .list_sessions(Default::default())
            .await
            .unwrap_or_default()
            .len()
    );
    println!(
        "Available models: {}",
        client.list_models().await.unwrap_or_default().len()
    );
    Ok(())
}

async fn execute_app_server_info_command() -> Result<(), Box<dyn std::error::Error>> {
    println!("Talos CLI - AI Assistant System");
    println!("Version: 1.0.0");
    println!("Transport: app-server JSON-RPC stdio");
    Ok(())
}

fn build_message_request(
    message: &str,
    _model: Option<String>,
    _stream: bool,
) -> TalosMessageRequest {
    TalosMessageRequest {
        input: vec![TalosInputItem::Text {
            text: message.to_string(),
        }],
        workspace_id: None,
    }
}

async fn send_and_render_app_server_message(
    client: &mut AppServerClient,
    session_id: &str,
    request: TalosMessageRequest,
) -> Result<String, Box<dyn std::error::Error>> {
    let mut output_text = String::new();
    let mut output_stream = std::io::stdout();
    let mut events = client.send_timeline_events(session_id, request).await?;
    let mut item_phases = HashMap::new();

    while let Some(envelope) = events.recv().await {
        if render_event(
            &envelope,
            &mut item_phases,
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
    output_text: &mut String,
    output_stream: &mut std::io::Stdout,
) -> Result<bool, Box<dyn std::error::Error>> {
    let event = &envelope.event;
    match &event.payload {
        TalosTimelineEventPayload::ItemStarted(payload) => {
            item_phases.insert(payload.item.item_id.clone(), payload.item.phase);
            render_started_item(&payload.item);
        }
        TalosTimelineEventPayload::ItemDelta(payload) => match &payload.delta {
            TalosTimelineDelta::Text { text } => {
                if item_phases.get(&payload.item_id) == Some(&TalosTimelinePhase::FinalAnswer) {
                    output_text.push_str(text);
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
            TalosTimelineDelta::ToolApprovalRequest {
                call_id, tool_name, ..
            } => {
                eprintln!(
                    "TOOL_CALL_APPROVAL tool={} internal={} call_id={}",
                    display_tool_name(tool_name),
                    tool_name,
                    call_id
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
            TalosTimelineDelta::Status { status } => {
                eprintln!("ITEM_STATUS item={} status={:?}", payload.item_id, status);
            }
        },
        TalosTimelineEventPayload::ItemCompleted(payload) => {
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
        }
        TalosTimelineEventPayload::TurnError(payload) => {
            error!("TURN_ERROR {}", payload.error);
            return Ok(true);
        }
        TalosTimelineEventPayload::TurnCompleted(_) => {
            return Ok(true);
        }
        TalosTimelineEventPayload::TurnStarted(_) => {}
    }
    Ok(false)
}

fn render_started_item(item: &talos_client::TalosTimelineItem) {
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

fn display_tool_name(tool_name: &str) -> &str {
    match tool_name {
        "bash" | "Bash" => "Bash",
        "read" | "Read" => "Read",
        "grep" | "Grep" => "Grep",
        "write" | "write_file" | "Write" => "Write",
        "edit" | "Edit" => "Edit",
        _ => tool_name,
    }
}

fn default_chat_context(user_id: &str) -> ProtocolTalosContext {
    ProtocolTalosContext {
        source_type: "cli".to_string(),
        provider: "cli".to_string(),
        channel_id: "cli".to_string(),
        thread_id: None,
        message_id: format!("msg-{}", uuid::Uuid::new_v4().simple()),
        trace_id: format!("trace-{}-{user_id}", uuid::Uuid::new_v4().simple()),
    }
}

fn current_cli_directory() -> Option<std::path::PathBuf> {
    std::env::current_dir().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_parsing() {
        let args = vec!["talos", "chat", "simple", "hello"];
        let cli = Cli::try_parse_from(args);
        assert!(cli.is_ok());
    }

    #[test]
    fn test_cli_stream_flag_parsing() {
        let args = vec!["talos", "chat", "in", "sess-1", "hello", "--stream"];
        let cli = Cli::try_parse_from(args);
        assert!(cli.is_ok());
    }

    #[test]
    fn test_cli_approve_tool_can_parse_rejection() {
        let args = vec![
            "talos",
            "session",
            "approve-tool",
            "sess-1",
            "--call-id",
            "call-1",
            "--tool-name",
            "bash",
            "--approved",
            "false",
            "--reason",
            "deny",
        ];
        let cli = Cli::try_parse_from(args).expect("CLI should parse approval rejection");

        let Commands::Session(SessionCommands::ApproveTool {
            session_id,
            call_id,
            tool_name,
            approved,
            reason,
        }) = cli.command
        else {
            panic!("expected approve-tool command");
        };

        assert_eq!(session_id, "sess-1");
        assert_eq!(call_id, "call-1");
        assert_eq!(tool_name, "bash");
        assert!(!approved);
        assert_eq!(reason.as_deref(), Some("deny"));
    }
}
