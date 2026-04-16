use serde::{Deserialize, Serialize};

use crate::TalosPaths;

/// Memory defaults under agent.kernel
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentKernelMemoryConfig {
    /// Workspace root directory for memory and session markdown files.
    #[serde(default = "default_kernel_memory_workspace_dir")]
    pub workspace_dir: String,

    /// In-memory hot recent message capacity.
    #[serde(default = "default_kernel_memory_hot_recent_messages")]
    pub hot_recent_messages: usize,

    /// Trigger summary compaction when session memory exceeds this threshold.
    #[serde(default = "default_kernel_memory_compaction_threshold_bytes")]
    pub compaction_threshold_bytes: usize,

    /// Root memory file name.
    #[serde(default = "default_kernel_memory_root_memory_filename")]
    pub root_memory_filename: String,

    /// Workspace memory directory name.
    #[serde(default = "default_kernel_memory_memory_dirname")]
    pub memory_dirname: String,

    /// Enable workspace memory flush.
    #[serde(default = "default_true")]
    pub workspace_memory_flush_enabled: bool,

    /// Enable workspace memory flush before compaction.
    #[serde(default = "default_true")]
    pub workspace_memory_flush_on_compaction: bool,

    /// Daily workspace memory file pattern.
    #[serde(default = "default_kernel_memory_flush_file_pattern")]
    pub flush_file_pattern: String,

    /// Enable memory search tool.
    #[serde(default = "default_true")]
    pub tool_search_enabled: bool,

    /// Enable memory get tool.
    #[serde(default = "default_true")]
    pub tool_get_enabled: bool,

    /// Default max results for workspace memory search.
    #[serde(default = "default_kernel_memory_search_max_results")]
    pub search_max_results: usize,

    /// Include root memory in workspace memory search.
    #[serde(default = "default_true")]
    pub search_include_root_memory: bool,

    /// Include daily memory files in workspace memory search.
    #[serde(default = "default_true")]
    pub search_include_daily_memory: bool,

    /// Include session transcripts in workspace memory search.
    #[serde(default)]
    pub search_include_sessions: bool,
}

fn default_kernel_memory_workspace_dir() -> String {
    TalosPaths::resolve()
        .workspace_dir_for_agent("talos")
        .to_string_lossy()
        .to_string()
}

fn default_kernel_memory_hot_recent_messages() -> usize {
    64
}

fn default_kernel_memory_compaction_threshold_bytes() -> usize {
    256 * 1024
}

fn default_kernel_memory_root_memory_filename() -> String {
    "MEMORY.md".to_string()
}

fn default_kernel_memory_memory_dirname() -> String {
    "memory".to_string()
}

fn default_kernel_memory_flush_file_pattern() -> String {
    "YYYY-MM-DD.md".to_string()
}

fn default_kernel_memory_search_max_results() -> usize {
    6
}

impl Default for AgentKernelMemoryConfig {
    fn default() -> Self {
        Self {
            workspace_dir: default_kernel_memory_workspace_dir(),
            hot_recent_messages: default_kernel_memory_hot_recent_messages(),
            compaction_threshold_bytes: default_kernel_memory_compaction_threshold_bytes(),
            root_memory_filename: default_kernel_memory_root_memory_filename(),
            memory_dirname: default_kernel_memory_memory_dirname(),
            workspace_memory_flush_enabled: true,
            workspace_memory_flush_on_compaction: true,
            flush_file_pattern: default_kernel_memory_flush_file_pattern(),
            tool_search_enabled: true,
            tool_get_enabled: true,
            search_max_results: default_kernel_memory_search_max_results(),
            search_include_root_memory: true,
            search_include_daily_memory: true,
            search_include_sessions: false,
        }
    }
}

fn default_true() -> bool {
    true
}
