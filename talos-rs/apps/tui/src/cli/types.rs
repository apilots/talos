use clap::{Args, Parser, Subcommand};

/// Command-line interface for Talos terminal command mode.
#[derive(Parser)]
#[command(name = "talos")]
#[command(bin_name = "talos")]
#[command(version)]
#[command(about = "Talos terminal command mode", long_about = None)]
pub(crate) struct Cli {
    /// Terminal command mode command.
    #[command(subcommand)]
    pub(crate) command: Option<Commands>,
}

/// Terminal command mode commands.
#[derive(Subcommand)]
pub(crate) enum Commands {
    /// Launch interactive TUI mode.
    Tui,

    /// Execute one task and stream the result to stdout.
    Exec(ExecCmd),

    /// Session management commands.
    #[command(subcommand)]
    Session(SessionCommands),

    /// Available models.
    Models(ModelsCmd),

    /// Manage Skills and MCP installations through the selected Worker's Plugin service.
    Plugin(PluginCmd),

    /// System status check.
    Status,
}

#[derive(Args)]
pub(crate) struct ExecCmd {
    /// Session ID to reuse. A new session is created when omitted.
    #[arg(long)]
    pub(crate) session: Option<String>,

    /// Provider-qualified model override (`provider:model`).
    #[arg(short, long)]
    pub(crate) model: Option<String>,

    /// User ID for the request context.
    #[arg(long)]
    pub(crate) user: Option<String>,

    /// Message to execute.
    #[arg(required = true, trailing_var_arg = true)]
    pub(crate) message: Vec<String>,
}

#[derive(Subcommand)]
pub(crate) enum SessionCommands {
    /// List all sessions.
    List {
        /// Number of recent sessions to show.
        #[arg(long, default_value = "10")]
        limit: usize,
    },

    /// Get session details.
    Info {
        /// Session ID to get information for.
        session_id: String,
    },

    /// Update session model.
    SetModel {
        /// Session ID to update.
        session_id: String,
        /// Provider-qualified model (`provider:model`).
        model: String,
    },
}

#[derive(Args)]
pub(crate) struct ModelsCmd {
    /// Output format. Supported values: table, json.
    #[arg(long, default_value = "table")]
    pub(crate) format: String,
}

#[derive(Args)]
pub(crate) struct PluginCmd {
    /// Target Computer ID. Omit to use this embedded Worker.
    #[arg(long, global = true)]
    pub(crate) node: Option<String>,
    #[command(subcommand)]
    pub(crate) command: PluginCommands,
}

#[derive(Subcommand)]
pub(crate) enum PluginCommands {
    /// List installed plugins, including disabled installations.
    List,
    /// Install a discovered plugin by its Plugin ID.
    Install {
        plugin_id: String,
        /// Require this source commit.
        #[arg(long)]
        commit: Option<String>,
        /// Install without enabling the plugin.
        #[arg(long)]
        disabled: bool,
        /// Replace an existing conflicting installation.
        #[arg(long)]
        replace: bool,
    },
    /// Update an installation from its existing source.
    Update {
        install_id: String,
        /// Require this source commit.
        #[arg(long)]
        commit: Option<String>,
        /// Replace local edits with the source contents.
        #[arg(long)]
        replace_local_changes: bool,
    },
    /// Enable an existing installation.
    Enable { install_id: String },
    /// Disable an existing installation.
    Disable { install_id: String },
    /// Uninstall through the existing Plugin lifecycle.
    Uninstall { install_id: String },
}
