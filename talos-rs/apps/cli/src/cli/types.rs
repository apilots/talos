use clap::{Args, Parser, Subcommand};

/// Command-line interface for Talos AI system - Full-featured command set for system access
#[derive(Parser)]
#[command(name = "talos-cli")]
#[command(bin_name = "talos-cli")]
#[command(version)]
#[command(about = "Talos AI System CLI - Full-stack access to AI assistant capabilities", long_about = None)]
pub struct Cli {
    /// App-server host binary.
    #[arg(long)]
    pub(super) app_server_bin: Option<String>,

    /// Commands for the CLI
    #[command(subcommand)]
    pub(super) command: Commands,
}

/// Major command groups in Talos CLI
#[derive(Subcommand)]
pub(super) enum Commands {
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

    /// Authentication commands
    #[command(subcommand)]
    Auth(AuthCommands),

    /// System information
    Info,

    /// Run a local app-server protocol smoke scenario
    AppServerSmoke(AppServerSmokeCmd),
}

#[derive(Subcommand)]
pub(super) enum AuthCommands {
    /// Start Cloud device login and wait until approved
    Login {
        /// Print the login URL without opening a browser
        #[arg(long, default_value_t = false)]
        no_browser: bool,
    },

    /// Show cached Cloud login status
    Status,

    /// Logout and clear cached Cloud session
    Logout,
}

#[derive(Subcommand)]
pub(super) enum ChatCommands {
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
        /// Render events as they arrive
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
        /// Render events as they arrive
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
        /// Render events as they arrive
        #[arg(long, default_value_t = false)]
        stream: bool,
    },
}

#[derive(Subcommand)]
pub(super) enum SessionCommands {
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
        /// Render events as they arrive
        #[arg(long, default_value_t = false)]
        stream: bool,
    },
}

#[derive(Args)]
pub(super) struct ModelsCmd {
    /// Output format (table, json, etc)
    #[arg(long, default_value = "table")]
    pub(super) format: String,
    /// Show verbose information
    #[arg(long)]
    pub(super) verbose: bool,
}

#[derive(Args)]
pub(super) struct AppServerSmokeCmd {
    /// Scenario name supported by talos-app-server-test-client
    #[arg(long, default_value = "turn-smoke")]
    pub(super) scenario: String,

    /// App-server host binary.
    #[arg(long)]
    pub(super) app_server_bin: Option<String>,
}
