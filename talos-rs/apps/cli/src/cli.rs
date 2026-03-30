//! Talos Command Line Interface
//!
//! This module provides a command-line interface for interacting with the Talos system
//! through the client library. It supports native model/session/message operations
//! via the new gateway API surface.

use clap::{Args, Parser, Subcommand};
use serde::{Deserialize, Serialize};
use std::io::Write;
use talos_client::{
    ClientConfig, SessionCreateParams, TalosClient, TalosContext, TalosEventPayload,
    TalosEventType, TalosInputItem, TalosMessageRequest, TalosMessageResponse, TalosOutputItem,
    TalosThinkingSource, TalosThinkingStage,
};

/// Command-line interface for Talos AI system - Full-featured command set for system access
#[derive(Parser)]
#[command(name = "talos")]
#[command(bin_name = "talos")]
#[command(version = "1.0")]
#[command(about = "Talos AI System CLI - Full-stack access to AI assistant capabilities", long_about = None)]
pub struct Cli {
    /// Gateway server URL (overrides config file)  
    #[arg(long)]
    gateway_url: Option<String>,

    /// Authentication token (overrides config file)
    #[arg(long)]
    auth_token: Option<String>,

    /// Commands for the CLI
    #[command(subcommand)]
    command: Commands,
}

/// Major command groups in Talos CLI
#[derive(Subcommand)]
enum Commands {
    /// Chat commands (simple and session-based chatting)
    #[command(subcommand)]
    Chat(ChatCommands),

    /// Session management commands
    #[command(subcommand)]
    Session(SessionCommands),

    /// Available models
    Models(ModelsCmd),

    /// System status check
    Status,

    /// System information
    Info,
}

#[derive(Subcommand)]
enum ChatCommands {
    /// Simple chat (traditional mode)
    Simple {
        /// Message to send
        message: String,
        /// Model to use
        #[arg(short, long)]
        model: Option<String>,
        /// User ID
        #[arg(long)]
        user: Option<String>,
        /// Use SSE stream response
        #[arg(long, default_value_t = false)]
        stream: bool,
    },

    /// Chat within a specific session  
    In {
        /// Session ID
        session_id: String,
        /// Message to send
        message: String,
        /// Model override for this request (optional)
        #[arg(short, long)]
        model: Option<String>,
        /// Use SSE stream response
        #[arg(long, default_value_t = false)]
        stream: bool,
    },

    /// Continue chat in recent session
    Recent {
        /// Recent session index (0 for latest)
        #[arg(default_value = "0")]
        index: usize,
        /// Message to send
        message: String,
        /// Model override
        #[arg(short, long)]
        model: Option<String>,
        /// Use SSE stream response
        #[arg(long, default_value_t = false)]
        stream: bool,
    },
}

#[derive(Subcommand)]
enum SessionCommands {
    /// Create a new session
    Create {
        /// Initial model for the session
        #[arg(short, long)]
        model: Option<String>,
        /// Session name
        #[arg(long)]
        name: Option<String>,
    },

    /// List all sessions
    List {
        /// Number of recent sessions to show
        #[arg(long, default_value = "10")]
        limit: usize,
        /// User ID filter
        #[arg(long)]
        user: Option<String>,
    },

    /// Get session details
    Info {
        /// Session ID to get information for
        session_id: String,
    },

    /// Update session model
    SetModel {
        /// Session ID to update
        session_id: String,
        /// New model
        model: String,
    },

    /// Interactive session with session-specific context
    Interactive {
        /// Session ID for interactive mode
        session_id: String,
        /// Model to use in this interactive session
        #[arg(short, long)]
        model: Option<String>,
        /// Use SSE stream response
        #[arg(long, default_value_t = false)]
        stream: bool,
    },
}

// 4. Models Commands
#[derive(Args)]
struct ModelsCmd {
    /// Output format (table, json, etc)
    #[arg(long, default_value = "table")]
    format: String,
    /// Show verbose information
    #[arg(long)]
    verbose: bool,
}

/// Configuration loaded from file  
#[derive(Deserialize, Serialize)]
struct TalosCliConfig {
    gateway_url: Option<String>,
    auth_token: Option<String>,
    default_model: Option<String>,
    default_agent: Option<String>,
}

impl TalosCliConfig {
    fn load_from_file(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        if std::path::Path::new(path).exists() {
            let content = std::fs::read_to_string(path)?;
            let config: TalosCliConfig = serde_yaml::from_str(&content)?;
            Ok(config)
        } else {
            Ok(Self {
                gateway_url: Some("http://localhost:8080".to_string()),
                auth_token: None,
                default_model: Some("gpt-3.5-turbo".to_string()),
                default_agent: Some("default".to_string()),
            })
        }
    }
}

/// Main function to run the CLI
pub async fn run_cli() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    // Default config path
    let file_config = TalosCliConfig::load_from_file("talos.yaml")?; // default config

    // Override with command-line arguments if provided
    let gateway_url = cli
        .gateway_url
        .or(file_config.gateway_url)
        .unwrap_or("http://localhost:8080".to_string());
    let auth_token = cli.auth_token.or(file_config.auth_token);

    // Create client configuration
    let client_config = ClientConfig {
        gateway_url,
        auth_token,
        timeout: 30,
    };

    // Create the client
    let client = TalosClient::new(client_config)?;

    // Execute the command
    match cli.command {
        Commands::Chat(cmd) => execute_chat_command(&client, cmd).await?,
        Commands::Session(cmd) => execute_session_command(&client, cmd).await?,
        Commands::Models(cmd) => execute_models_command(&client, cmd).await?,
        Commands::Status => execute_status_command(&client).await?,
        Commands::Info => execute_info_command(&client).await?,
    }

    Ok(())
}

// Execute chat commands
async fn execute_chat_command(
    client: &TalosClient,
    cmd: ChatCommands,
) -> Result<(), Box<dyn std::error::Error>> {
    match cmd {
        ChatCommands::Simple {
            message,
            model,
            user,
            stream,
        } => {
            let params = SessionCreateParams {
                context: Some(default_chat_context(user.as_deref().unwrap_or("cli-user"))),
                model: model.clone(),
            };
            let session = client.create_session(&params).await?;
            let request = build_message_request(&message, model, stream);
            send_and_render_message(client, &session.context.session_id, &request, stream).await?;
        }
        ChatCommands::In {
            session_id,
            message,
            model,
            stream,
        } => {
            let request = build_message_request(&message, model, stream);
            send_and_render_message(client, &session_id, &request, stream).await?;
        }
        ChatCommands::Recent {
            index,
            message,
            model,
            stream,
        } => {
            let session_id = client
                .list_sessions()
                .await
                .ok()
                .and_then(|sessions| sessions.into_iter().nth(index))
                .map(|s| s.context.session_id);

            if let Some(session_id) = session_id {
                let request = build_message_request(&message, model, stream);
                send_and_render_message(client, &session_id, &request, stream).await?;
            } else {
                eprintln!("No recent session found at index {}", index);
            }
        }
    }
    Ok(())
}

// Execute session commands
async fn execute_session_command(
    client: &TalosClient,
    cmd: SessionCommands,
) -> Result<(), Box<dyn std::error::Error>> {
    match cmd {
        SessionCommands::Create { model, name: _ } => {
            let params = SessionCreateParams {
                context: None,
                model,
            };
            let session = client.create_session(&params).await?;
            println!(
                "Session created: {} (model: {})",
                session.context.session_id, session.config.llm.model
            );
        }
        SessionCommands::List { limit, user: _ } => {
            let sessions = client.list_sessions().await?;
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
            client.update_session_model(&session_id, &model).await?;
            let session = client.get_session(&session_id).await?;
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

            // Simple stdin loop
            loop {
                use std::io::{self, BufRead};
                print!("> ");
                io::stdout().flush().unwrap();

                let stdin = io::stdin();
                let mut line = String::new();
                stdin.lock().read_line(&mut line).unwrap();

                let input = line.trim();
                if input == "quit" || input == "exit" {
                    if let Err(err) = client.close_session(&session_id).await {
                        eprintln!("Failed to close session {}: {}", session_id, err);
                    }
                    break;
                }

                if !input.is_empty() {
                    let request = build_message_request(input, model.clone(), stream);
                    match send_and_render_message(client, &session_id, &request, stream).await {
                        Ok(_) => {}
                        Err(e) => eprintln!("Error: {:?}", e),
                    };
                }
            }
            println!("Exited interactive mode");
        }
    }
    Ok(())
}

// Execute models commands
async fn execute_models_command(
    client: &TalosClient,
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

// Execute status command
async fn execute_status_command(client: &TalosClient) -> Result<(), Box<dyn std::error::Error>> {
    println!("Status: Gateway accessible");
    println!(
        "Active sessions: {}",
        client.list_sessions().await.unwrap_or_default().len()
    );
    println!(
        "Available models: {}",
        client.list_models().await.unwrap_or_default().len()
    );
    Ok(())
}

// Execute info command
async fn execute_info_command(_client: &TalosClient) -> Result<(), Box<dyn std::error::Error>> {
    println!("Talos CLI - AI Assistant System");
    println!("Version: 1.0.0");
    println!("Gateway: configured via --gateway-url or talos.yaml");
    Ok(())
}

fn build_message_request(
    message: &str,
    _model: Option<String>,
    stream: bool,
) -> TalosMessageRequest {
    TalosMessageRequest {
        input: vec![TalosInputItem::Text {
            text: message.to_string(),
        }],
        stream: Some(stream),
        workspace_id: None,
    }
}

async fn send_and_render_message(
    client: &TalosClient,
    session_id: &str,
    request: &TalosMessageRequest,
    stream: bool,
) -> Result<String, Box<dyn std::error::Error>> {
    if !stream {
        let response = client.send_message(session_id, request).await?;
        for item in &response.output {
            match item {
                TalosOutputItem::Reasoning { text } => {
                    let trimmed = text.trim();
                    if !trimmed.is_empty() {
                        eprintln!("REASONING {}", trimmed);
                    }
                }
                TalosOutputItem::ExecutionSummary { text } => {
                    let trimmed = text.trim();
                    if !trimmed.is_empty() {
                        eprintln!("SUMMARY {}", trimmed);
                    }
                }
                TalosOutputItem::ToolCall { call_id, name, .. } => {
                    eprintln!("TOOL_CALL name={} call_id={}", name, call_id);
                }
                TalosOutputItem::ToolResult {
                    call_id, is_error, ..
                } => {
                    eprintln!("TOOL_RESULT call_id={} is_error={}", call_id, is_error);
                }
                TalosOutputItem::OutputText { .. } => {}
            }
        }
        let text = extract_output_text(&response);
        println!("{}", text);
        return Ok(text);
    }

    let mut output_text = String::new();
    let mut output_stream = std::io::stdout();
    let mut events = client.send_message_stream(session_id, request).await?;

    while let Some(event) = events.recv().await {
        match (&event.event_type, &event.payload) {
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
                    "TOOL_CALL_STARTED name={} call_id={} step={}",
                    payload.name, payload.call_id, payload.step_id
                );
            }
            (
                TalosEventType::MessageToolCallCompleted,
                TalosEventPayload::MessageToolCallCompleted(payload),
            ) => {
                eprintln!(
                    "TOOL_CALL_COMPLETED call_id={} tool={} is_error={}",
                    payload.call_id, payload.tool, payload.is_error
                );
            }
            (TalosEventType::MessageError, TalosEventPayload::MessageError(payload)) => {
                eprintln!("MESSAGE_ERROR {}", payload.error);
            }
            (TalosEventType::MessageStarted, TalosEventPayload::MessageStarted(_))
            | (TalosEventType::MessageCompleted, TalosEventPayload::MessageCompleted(_)) => {}
            _ => {}
        }
    }

    if !output_text.is_empty() && !output_text.ends_with('\n') {
        println!();
    }

    Ok(output_text)
}

fn extract_output_text(response: &TalosMessageResponse) -> String {
    let texts: Vec<String> = response
        .output
        .iter()
        .filter_map(|item| match item {
            TalosOutputItem::OutputText { text } => Some(text.clone()),
            _ => None,
        })
        .collect();

    if texts.is_empty() {
        return serde_json::to_string(&response.output).unwrap_or_default();
    }

    texts.join("\n")
}

fn default_chat_context(user_id: &str) -> TalosContext {
    TalosContext::cli(user_id).with_channel_id("cli")
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
}
