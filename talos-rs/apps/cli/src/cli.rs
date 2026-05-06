//! Talos Command Line Interface
//!
//! This module provides a command-line interface for interacting with the Talos system
//! through the local JSON-RPC app-server client.

use clap::Parser;
use std::io::Write;
use talos_client::{
    AppServerClient, AppServerClientConfig, EventSubscribeParams, ProtocolTalosContext,
    SessionCreateParams, TalosEventPayload, TalosEventType, TalosInputItem, TalosMessageRequest,
    TalosThinkingSource, TalosThinkingStage,
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
    let subscription = client
        .subscribe_events(EventSubscribeParams {
            session_id: session_id.clone(),
            since_event_id: None,
            replay_limit: Some(32),
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
        let event = client.next_event().await?;
        if event.subscription_id != subscription.subscription_id {
            continue;
        }
        match event.event.event_type {
            TalosEventType::MessageStarted => saw_started = true,
            TalosEventType::MessageOutputDelta => saw_delta = true,
            TalosEventType::MessageCompleted if saw_started && saw_delta => return Ok(()),
            TalosEventType::MessageError => {
                let error = match event.event.payload {
                    TalosEventPayload::MessageError(payload) => payload.error,
                    _ => "unknown app-server error".to_string(),
                };
                return Err(format!("app-server turn failed: {error}").into());
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
    let subscription = client
        .subscribe_events(EventSubscribeParams {
            session_id: session_id.clone(),
            since_event_id: None,
            replay_limit: Some(128),
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
    let mut pending_sequence = None;
    for _ in 0..1024 {
        let event = client.next_event().await?;
        if event.subscription_id != subscription.subscription_id {
            continue;
        }
        match event.event.payload {
            TalosEventPayload::MessageToolCallPendingApproval(payload) => {
                pending_call_id = Some(payload.call_id);
                pending_tool_name = Some(payload.tool_name);
                pending_event_id = Some(event.event_id.clone());
                pending_sequence = Some(parse_app_server_event_sequence(&event.event_id)?);
            }
            TalosEventPayload::MessageCompleted(_) if pending_call_id.is_some() => {
                break;
            }
            TalosEventPayload::MessageError(payload) => {
                return Err(format!("app-server approval setup failed: {}", payload.error).into());
            }
            _ => {}
        }
    }

    let call_id = pending_call_id.ok_or("missing pending approval call_id")?;
    let tool_name = pending_tool_name.ok_or("missing pending approval tool_name")?;
    let event_id = pending_event_id.ok_or("missing pending approval event_id")?;
    let after_sequence = pending_sequence.ok_or("missing pending approval sequence")?;
    let _live_resume_subscription = client
        .subscribe_events(EventSubscribeParams {
            session_id: session_id.clone(),
            since_event_id: None,
            replay_limit: Some(128),
        })
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
    let resumed_subscription = client
        .subscribe_events(EventSubscribeParams {
            session_id: session_id.clone(),
            since_event_id: Some(event_id),
            replay_limit: Some(512),
        })
        .await?;

    let mut saw_tool_completed = false;
    for _ in 0..1024 {
        let event = client.next_event().await?;
        if event.subscription_id != resumed_subscription.subscription_id
            || parse_app_server_event_sequence(&event.event_id)? <= after_sequence
        {
            continue;
        }
        match event.event.payload {
            TalosEventPayload::MessageToolCallCompleted(payload) if payload.call_id == call_id => {
                saw_tool_completed = true;
            }
            TalosEventPayload::MessageCompleted(_) if saw_tool_completed => return Ok(()),
            TalosEventPayload::MessageError(payload) => {
                return Err(format!("app-server approval resume failed: {}", payload.error).into());
            }
            _ => {}
        }
    }

    Err("app-server approval resume did not complete approved tool call".into())
}

fn parse_app_server_event_sequence(event_id: &str) -> Result<u64, Box<dyn std::error::Error>> {
    Ok(event_id
        .strip_prefix("ev_")
        .ok_or_else(|| format!("invalid event id prefix: {event_id}"))?
        .parse::<u64>()?)
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
    let mut events = client.send_message_events(session_id, request).await?;

    while let Some(event) = events.recv().await {
        if render_event(
            &event.event_type,
            &event.payload,
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
    event_type: &TalosEventType,
    payload: &TalosEventPayload,
    output_text: &mut String,
    output_stream: &mut std::io::Stdout,
) -> Result<bool, Box<dyn std::error::Error>> {
    match (event_type, payload) {
        (
            TalosEventType::MessageReasoningDelta,
            TalosEventPayload::MessageReasoningDelta(payload),
        ) => {
            if !payload.delta.trim().is_empty() {
                eprintln!(
                    "THINKING[{}:{}][iter={}][{}] {}",
                    format_thinking_source(payload.source),
                    format_thinking_stage(payload.stage),
                    payload.iteration,
                    payload.step_id,
                    payload.delta
                );
            }
        }
        (
            TalosEventType::MessageExecutionSummary,
            TalosEventPayload::MessageExecutionSummary(payload),
        ) => {
            if !payload.text.trim().is_empty() {
                eprintln!(
                    "SUMMARY[iter={}][{}] {}",
                    payload.iteration, payload.step_id, payload.text
                );
            }
        }
        (TalosEventType::MessageOutputDelta, TalosEventPayload::MessageOutputDelta(payload)) => {
            output_text.push_str(&payload.delta);
            write!(output_stream, "{}", payload.delta)?;
            output_stream.flush()?;
        }
        (
            TalosEventType::MessageToolCallStarted,
            TalosEventPayload::MessageToolCallStarted(payload),
        ) => {
            eprintln!(
                "TOOL_CALL_STARTED name={} internal={} call_id={} step={}",
                display_tool_name(&payload.name),
                payload.name,
                payload.call_id,
                payload.step_id
            );
        }
        (
            TalosEventType::MessageToolCallArgsDelta,
            TalosEventPayload::MessageToolCallArgsDelta(payload),
        ) => {
            eprintln!(
                "TOOL_CALL_ARGS_DELTA tool={} internal={} call_id={} step={} delta={}",
                payload
                    .tool_name
                    .as_deref()
                    .map(display_tool_name)
                    .unwrap_or("unknown"),
                payload.tool_name.as_deref().unwrap_or("unknown"),
                payload.call_id.as_deref().unwrap_or("pending"),
                payload.step_id,
                payload.delta
            );
        }
        (
            TalosEventType::MessageToolCallQueued,
            TalosEventPayload::MessageToolCallQueued(payload),
        ) => {
            eprintln!(
                "TOOL_CALL_QUEUED tool={} internal={} call_id={} step={}",
                display_tool_name(&payload.tool_name),
                payload.tool_name,
                payload.call_id,
                payload.step_id
            );
        }
        (
            TalosEventType::MessageToolCallExecuting,
            TalosEventPayload::MessageToolCallExecuting(payload),
        ) => {
            eprintln!(
                "TOOL_CALL_EXECUTING tool={} internal={} call_id={} step={}",
                display_tool_name(&payload.tool_name),
                payload.tool_name,
                payload.call_id,
                payload.step_id
            );
        }
        (
            TalosEventType::MessageToolCallProgressDelta,
            TalosEventPayload::MessageToolCallProgressDelta(payload),
        ) => {
            eprintln!(
                "TOOL_CALL_PROGRESS tool={} internal={} call_id={} kind={:?} step={} message={}",
                display_tool_name(&payload.tool_name),
                payload.tool_name,
                payload.call_id,
                payload.progress_kind,
                payload.step_id,
                payload.message
            );
        }
        (
            TalosEventType::MessageToolCallCompleted,
            TalosEventPayload::MessageToolCallCompleted(payload),
        ) => {
            eprintln!(
                "TOOL_CALL_COMPLETED call_id={} tool={} internal={} is_error={}",
                payload.call_id,
                display_tool_name(&payload.tool),
                payload.tool,
                payload.is_error
            );
        }
        (TalosEventType::MessageError, TalosEventPayload::MessageError(payload)) => {
            error!("MESSAGE_ERROR {}", payload.error);
            return Ok(true);
        }
        (TalosEventType::MessageCompleted, TalosEventPayload::MessageCompleted(_)) => {
            return Ok(true);
        }
        (TalosEventType::MessageStarted, TalosEventPayload::MessageStarted(_)) => {}
        _ => {}
    }
    Ok(false)
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

fn format_thinking_source(source: TalosThinkingSource) -> &'static str {
    match source {
        TalosThinkingSource::Provider => "provider",
        TalosThinkingSource::Framework => "framework",
    }
}

fn format_thinking_stage(stage: TalosThinkingStage) -> &'static str {
    match stage {
        TalosThinkingStage::ModelReasoning => "model_reasoning",
        TalosThinkingStage::FrameworkPlan => "framework_plan",
        TalosThinkingStage::FrameworkAct => "framework_act",
        TalosThinkingStage::FrameworkObserve => "framework_observe",
        TalosThinkingStage::FrameworkReflect => "framework_reflect",
    }
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
