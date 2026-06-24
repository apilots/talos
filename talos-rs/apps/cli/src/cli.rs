//! Talos Command Line Interface
//!
//! This module provides a command-line interface for interacting with the Talos system
//! through the local JSON-RPC app-server client.

use clap::Parser;
use std::collections::HashMap;
use std::io::Write;
use std::time::Duration;
use talos_sdk::{
    now_ms, open_external_url, AppServerClient, AppServerClientConfig, CloudLoginClientKind,
    CloudLoginStatusResponse, SkillSelector, SkillsListRequest, SkillsReadRequest,
    SkillsRefreshRequest, SkillsSuggestRequest, StartTurnRequest, TalosContext, TalosInputItem,
    TalosProvider, TalosSourceType, TalosTimelineContent, TalosTimelineDelta,
    TalosTimelineEventPayload, TalosTimelineItemKind, TalosTimelinePhase, TimelineEventEnvelope,
};
use tracing::{debug, error};

mod types;

use types::{
    AppServerSmokeCmd, AuthCommands, ChatCommands, Cli, Commands, ModelsCmd, SessionCommands,
    SkillsCommands,
};

/// Main function to run the CLI
pub async fn run_cli() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    run_app_server_cli(cli).await
}

async fn run_app_server_cli(cli: Cli) -> Result<(), Box<dyn std::error::Error>> {
    let app_server_bin = cli
        .app_server_bin
        .or_else(|| std::env::var("TALOS_APP_SERVER_BIN").ok())
        .unwrap_or_else(|| "app-server".to_string());
    let config = AppServerClientConfig {
        app_server_bin,
        client_name: "talos-cli".to_string(),
        client_version: env!("CARGO_PKG_VERSION").to_string(),
        actor_credentials: None,
        use_cloud_session_cache: false,
    }
    .with_cloud_session_cache();
    let mut client = AppServerClient::connect(config).await?;

    let result = match cli.command {
        Commands::Chat(cmd) => execute_app_server_chat_command(&mut client, cmd).await,
        Commands::Session(cmd) => execute_app_server_session_command(&mut client, cmd).await,
        Commands::Models(cmd) => execute_app_server_models_command(&mut client, cmd).await,
        Commands::Skills(cmd) => execute_app_server_skills_command(&mut client, cmd).await,
        Commands::Status => execute_app_server_status_command(&mut client).await,
        Commands::Auth(cmd) => execute_auth_command(&mut client, cmd).await,
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

async fn execute_app_server_skills_command(
    client: &mut AppServerClient,
    cmd: SkillsCommands,
) -> Result<(), Box<dyn std::error::Error>> {
    match cmd {
        SkillsCommands::List { include_disabled } => {
            let response = client
                .list_skills(SkillsListRequest { include_disabled })
                .await?;
            print_skills_snapshot(response);
        }
        SkillsCommands::Refresh => {
            let response = client.refresh_skills(SkillsRefreshRequest {}).await?;
            print_skills_snapshot(response);
        }
        SkillsCommands::Suggest { query, limit } => {
            let response = client
                .suggest_skills(SkillsSuggestRequest {
                    query,
                    limit: Some(limit),
                })
                .await?;
            for skill in response.data {
                println!(
                    "${} [{}] {}",
                    skill.name,
                    format!("{:?}", skill.scope).to_lowercase(),
                    skill.description
                );
            }
        }
        SkillsCommands::Read { name, path } => {
            let selector = SkillSelector {
                name,
                path: path.map(Into::into),
            };
            if selector.name.is_none() && selector.path.is_none() {
                return Err("skills read requires --name or --path".into());
            }
            let response = client.read_skill(SkillsReadRequest { selector }).await?;
            let document = response.document;
            println!("# {}", document.summary.name);
            println!("Path: {}", document.summary.path.display());
            println!();
            println!("{}", document.contents);
        }
    }
    Ok(())
}

fn print_skills_snapshot(response: talos_sdk::SkillsListResponse) {
    for skill in response.data {
        println!(
            "${} [{}] {}{}",
            skill.name,
            format!("{:?}", skill.scope).to_lowercase(),
            skill.description,
            if skill.enabled { "" } else { " (disabled)" }
        );
    }
    for error in response.errors {
        eprintln!(
            "load error: root={} path={} message={}",
            error.root.display(),
            error
                .path
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "-".to_string()),
            error.message
        );
    }
}

async fn execute_auth_command(
    client: &mut AppServerClient,
    cmd: AuthCommands,
) -> Result<(), Box<dyn std::error::Error>> {
    match cmd {
        AuthCommands::Login { no_browser } => login_with_cloud(client, no_browser).await,
        AuthCommands::Status => {
            let response = client.read_auth_session().await?;
            if let Some(session) = response.session {
                println!("Authenticated as {}", session.user.username);
                println!("User ID: {}", session.user.user_id);
                println!("Cloud API: {}", session.cloud_api_base_url);
            } else {
                println!("Not authenticated");
            }
            Ok(())
        }
        AuthCommands::Logout => {
            client.auth_logout().await?;
            println!("Logged out");
            Ok(())
        }
    }
}

async fn login_with_cloud(
    client: &mut AppServerClient,
    no_browser: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let start = client
        .start_cloud_login("Talos CLI", CloudLoginClientKind::Cli)
        .await?;
    println!("Open this URL to sign in:");
    println!("{}", start.login_url);
    println!("Code: {}", start.user_code);
    if !no_browser {
        if let Err(error) = open_external_url(&start.login_url) {
            eprintln!("Could not open browser automatically: {error}");
        }
    }

    loop {
        if now_ms() >= start.expires_at_ms {
            return Err("Cloud login expired".into());
        }
        match client.poll_cloud_login(start.login_id.clone()).await? {
            CloudLoginStatusResponse::Pending { interval_ms } => {
                tokio::time::sleep(Duration::from_millis(interval_ms.max(500))).await;
            }
            CloudLoginStatusResponse::Authenticated { user, .. } => {
                println!("Authenticated as {}", user.username);
                return Ok(());
            }
            CloudLoginStatusResponse::Denied => return Err("Cloud login was denied".into()),
            CloudLoginStatusResponse::Expired => return Err("Cloud login expired".into()),
        }
    }
}

async fn execute_app_server_smoke_with_client(
    client: &mut AppServerClient,
    cmd: AppServerSmokeCmd,
) -> Result<(), Box<dyn std::error::Error>> {
    match cmd.scenario.as_str() {
        "smoke" => run_app_server_smoke(client).await?,
        "turn-smoke" => run_app_server_turn_smoke(client).await?,
        other => return Err(format!("unsupported app-server smoke scenario: {other}").into()),
    }
    println!("App-server scenario passed: {}", cmd.scenario);
    Ok(())
}

async fn run_app_server_smoke(
    client: &mut AppServerClient,
) -> Result<(), Box<dyn std::error::Error>> {
    client.list_models().await?;
    client.config_get().await?;
    client.list_sessions(Default::default()).await?;
    Ok(())
}

async fn run_app_server_turn_smoke(
    client: &mut AppServerClient,
) -> Result<(), Box<dyn std::error::Error>> {
    let session_id = uuid::Uuid::new_v4().to_string();
    let mut events = client
        .send_timeline_events(StartTurnRequest {
            session_id: session_id.clone(),
            input: vec![TalosInputItem::Text {
                text: "answer with ok".to_string(),
            }],
            workspace_id: None,
            context: Some(default_chat_context("cli-app-server")),
            model: None,
            project_id: None,
            current_directory: current_cli_directory(),
            client_request_id: None,
        })
        .await?;

    let mut saw_started = false;
    let mut saw_delta = false;
    for _ in 0..128 {
        let Some(envelope) = events.recv().await else {
            break;
        };
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
            let session_id = uuid::Uuid::new_v4().to_string();
            let mut request = build_start_turn_request(&session_id, &message, model, stream);
            request.context = Some(default_chat_context(user.as_deref().unwrap_or("cli-user")));
            request.current_directory = current_cli_directory();
            send_and_render_app_server_message(client, request).await?;
        }
        ChatCommands::In {
            session_id,
            message,
            model,
            stream,
        } => {
            let mut request = build_start_turn_request(&session_id, &message, model, stream);
            request.context = Some(default_chat_context("cli-user"));
            request.current_directory = current_cli_directory();
            send_and_render_app_server_message(client, request).await?;
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
                .and_then(|response| response.sessions.into_iter().nth(index))
                .map(|session| session.id);
            if let Some(session_id) = session_id {
                let request = build_start_turn_request(&session_id, &message, model, stream);
                send_and_render_app_server_message(client, request).await?;
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
        SessionCommands::List { limit, user: _ } => {
            let sessions = client.list_sessions(Default::default()).await?;
            println!("Active Sessions:");
            for (i, session) in sessions.sessions.iter().take(limit).enumerate() {
                println!(
                    "{}. ID: {} | Model: {} | Messages: {}",
                    i, session.id, session.model, session.message_count
                );
            }
        }
        SessionCommands::Info { session_id } => {
            let response = client.get_session_by_id(&session_id).await?;
            let session = response.session;
            println!("Session Info:");
            println!("  ID: {}", session.id);
            println!("  Model: {}", session.model);
            println!("  Messages: {}", session.message_count);
        }
        SessionCommands::SetModel { session_id, model } => {
            let response = client.update_session_model(&session_id, &model).await?;
            let session = response.session;
            println!("Session model updated:");
            println!("  ID: {}", session.id);
            println!("  Model: {}", session.model);
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
                    if let Err(err) = client.delete_session_by_id(&session_id).await {
                        debug!("Failed to close session {}: {}", session_id, err);
                    }
                    break;
                }
                if !input.is_empty() {
                    let mut request =
                        build_start_turn_request(&session_id, input, model.clone(), stream);
                    request.context = Some(default_chat_context("cli-user"));
                    request.current_directory = current_cli_directory();
                    if let Err(err) = send_and_render_app_server_message(client, request).await {
                        debug!("Error: {:?}", err);
                    }
                }
            }
            println!("Exited interactive mode");
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
        for (i, model) in models.models.iter().enumerate() {
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
            .map(|response| response.sessions.len())
            .unwrap_or_default()
    );
    println!(
        "Available models: {}",
        client
            .list_models()
            .await
            .map(|response| response.models.len())
            .unwrap_or_default()
    );
    Ok(())
}

async fn execute_app_server_info_command() -> Result<(), Box<dyn std::error::Error>> {
    println!("Talos CLI - AI Assistant System");
    println!("Version: 1.0.0");
    println!("Transport: app-server JSON-RPC stdio");
    Ok(())
}

fn build_start_turn_request(
    session_id: &str,
    message: &str,
    model: Option<String>,
    _stream: bool,
) -> StartTurnRequest {
    StartTurnRequest {
        session_id: session_id.to_string(),
        input: vec![TalosInputItem::Text {
            text: message.to_string(),
        }],
        workspace_id: None,
        context: None,
        model,
        project_id: None,
        current_directory: None,
        client_request_id: None,
    }
}

async fn send_and_render_app_server_message(
    client: &mut AppServerClient,
    request: StartTurnRequest,
) -> Result<String, Box<dyn std::error::Error>> {
    let mut output_text = String::new();
    let mut output_stream = std::io::stdout();
    let mut events = client.send_timeline_events(request).await?;
    let mut item_phases = HashMap::new();
    let mut item_texts = HashMap::new();

    while let Some(envelope) = events.recv().await {
        if render_event(
            &envelope,
            &mut item_phases,
            &mut item_texts,
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
    output_text: &mut String,
    output_stream: &mut std::io::Stdout,
) -> Result<bool, Box<dyn std::error::Error>> {
    let event = &envelope.event;
    match &event.payload {
        TalosTimelineEventPayload::ItemStarted(payload) => {
            item_phases.insert(payload.item.item_id.clone(), payload.item.phase);
            if let TalosTimelineContent::Text { text } = &payload.item.content {
                item_texts.insert(payload.item.item_id.clone(), text.clone());
            }
            render_started_item(&payload.item);
        }
        TalosTimelineEventPayload::ItemDelta(payload) => match &payload.delta {
            TalosTimelineDelta::Text { text } => {
                item_texts
                    .entry(payload.item_id.clone())
                    .or_default()
                    .push_str(text);
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
        TalosTimelineEventPayload::AssistantTextCommitted(payload) => {
            item_phases.insert(payload.item_id.clone(), payload.phase);
            if payload.phase == TalosTimelinePhase::FinalAnswer {
                if let Some(text) = item_texts.get(&payload.item_id) {
                    output_text.push_str(text);
                    write!(output_stream, "{}", text)?;
                    output_stream.flush()?;
                }
            }
        }
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
        TalosTimelineEventPayload::TurnMetrics(_) => {}
        TalosTimelineEventPayload::TurnStarted(_) => {}
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

fn default_chat_context(user_id: &str) -> TalosContext {
    TalosContext {
        source_type: TalosSourceType::Cli,
        provider: TalosProvider::Cli,
        user_id: user_id.to_string(),
        channel_id: Some("cli".to_string()),
        thread_id: None,
        message_id: format!("msg-{}", uuid::Uuid::new_v4().simple()),
        trace_id: format!("trace-{}-{user_id}", uuid::Uuid::new_v4().simple()),
        project_root: None,
        environment: None,
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
}
