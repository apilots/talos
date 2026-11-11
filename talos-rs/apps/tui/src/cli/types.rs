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

    /// Continue one existing Session toward an objective.
    Goal(GoalCmd),
    /// Follow up in one existing Session.
    Loop(LoopCmd),
    /// Manage Session automation and owner-scoped native Jobs.
    Automation(AutomationCmd),
    /// System status check.
    Status,
}

#[derive(Args)]
#[command(
    after_help = "Exit codes: 0 completed, 1 execution failed, 2 invalid arguments, 3 waiting for input."
)]
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

    /// Save Full Access for the session. Use only in disposable environments.
    #[arg(long)]
    pub(crate) dangerously_skip_permissions: bool,

    /// Write execution measurements and terminal trajectory metadata to a new JSON file.
    #[arg(long, value_name = "FILE")]
    pub(crate) trajectory: Option<std::path::PathBuf>,

    /// Include user and assistant content, tool arguments, and results in the trajectory.
    #[arg(long, requires = "trajectory")]
    pub(crate) trajectory_include_content: bool,

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
    /// Manage Skill plugins by name.
    Skill {
        #[command(subcommand)]
        command: TypedPluginCommand,
    },
    /// Manage MCP plugins by name.
    Mcp {
        #[command(subcommand)]
        command: TypedPluginCommand,
    },
    /// Manage plugin sources.
    Marketplace {
        #[command(subcommand)]
        command: MarketplaceCommand,
    },
    /// List installed plugins, including disabled installations.
    List,
    /// Import and enable one MCP server from an MCP JSON file.
    ImportMcp { file: std::path::PathBuf },
    #[command(flatten)]
    Installation(PluginInstallationCommand),
}

#[derive(Subcommand)]
pub(crate) enum PluginInstallationCommand {
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

#[derive(Subcommand)]
pub(crate) enum TypedPluginCommand {
    /// Install a catalog entry or a local package.
    Add {
        /// Plugin name, optionally qualified as name@marketplace.
        #[arg(required_unless_present = "file", conflicts_with = "file")]
        name: Option<String>,
        /// ZIP or MCP JSON file on the selected Worker.
        #[arg(long)]
        file: Option<std::path::PathBuf>,
        /// Replace an existing installation with the same name.
        #[arg(long)]
        replace: bool,
    },
    /// List installations of this type.
    List,
    /// Remove an installation by name.
    Remove { name: String },
    /// Enable an installation by name.
    Enable { name: String },
    /// Disable an installation by name.
    Disable { name: String },
    /// Update an installation from its marketplace by name.
    Update { name: String },
}

#[derive(Subcommand)]
pub(crate) enum MarketplaceCommand {
    /// Add a Git repository or a directory on the selected Worker.
    Add {
        location: String,
        #[arg(long)]
        reference: Option<String>,
    },
    /// List configured marketplace sources.
    List,
    /// Refresh a marketplace by name or source ID.
    Refresh { marketplace: String },
    /// Remove a marketplace by name or source ID.
    Remove { marketplace: String },
}

#[derive(Args)]
pub(crate) struct GoalCmd {
    /// Existing Session. Automation never creates a cross-Session scheduler.
    #[arg(long)]
    pub(crate) session: String,
    #[command(subcommand)]
    pub(crate) command: Option<GoalCommands>,
}
#[derive(Subcommand)]
pub(crate) enum GoalCommands {
    Show,
    /// Run this Goal in the foreground; keep this embedded owner process alive.
    Start {
        #[arg(required = true, trailing_var_arg = true)]
        prompt: Vec<String>,
        #[arg(long)]
        token_budget: Option<u64>,
    },
    Edit {
        #[arg(required = true, trailing_var_arg = true)]
        prompt: Vec<String>,
    },
    Pause,
    /// Resume and observe the existing Goal in the foreground.
    Resume,
    Clear {
        #[arg(long)]
        id: Option<String>,
    },
}
#[derive(Args)]
pub(crate) struct LoopCmd {
    #[arg(long)]
    pub(crate) session: String,
    #[command(subcommand)]
    pub(crate) command: Option<LoopCommands>,
}
#[derive(Subcommand)]
pub(crate) enum LoopCommands {
    List,
    /// Run this Loop in the foreground; keep this embedded owner process alive.
    Start {
        /// Integer minutes, validated by the shared LoopSchedule contract.
        #[arg(long, conflicts_with = "dynamic")]
        every: Option<u32>,
        #[arg(long)]
        dynamic: bool,
        #[arg(required = true, trailing_var_arg = true)]
        prompt: Vec<String>,
    },
    Edit {
        id: String,
        #[arg(long, conflicts_with = "dynamic")]
        every: Option<u32>,
        #[arg(long)]
        dynamic: bool,
        #[arg(trailing_var_arg = true)]
        prompt: Vec<String>,
    },
    Stop {
        id: String,
    },
    Clear {
        id: String,
    },
}
#[derive(Args)]
pub(crate) struct AutomationCmd {
    #[command(subcommand)]
    pub(crate) command: AutomationCommands,
}
#[derive(Subcommand)]
pub(crate) enum AutomationCommands {
    /// Caller-owned Session tasks; pass the returned cursor for another page.
    List {
        #[arg(long)]
        session: Option<String>,
        #[arg(long)]
        ended: bool,
        #[arg(long)]
        cursor: Option<String>,
    },
    /// Native maintenance callbacks belonging to this actual Computer/instance.
    Jobs {
        #[arg(long)]
        computer: String,
        #[command(subcommand)]
        command: Option<JobCommands>,
    },
}
#[derive(Subcommand)]
pub(crate) enum JobCommands {
    List,
    Pause { id: String },
    Resume { id: String },
    RunOnce { id: String },
}
