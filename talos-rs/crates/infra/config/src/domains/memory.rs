use serde::{Deserialize, Serialize};

/// Global long-term memory file layout and file-list limits.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryConfig {
    /// Root memory file name.
    #[serde(default = "default_memory_root_filename")]
    pub root_memory_filename: String,

    /// Workspace memory directory name.
    #[serde(default = "default_memory_dirname")]
    pub memory_dirname: String,

    /// Default page size for memory file listing.
    #[serde(default = "default_memory_file_list_default_limit")]
    pub file_list_default_limit: usize,

    /// Max page size for memory file listing.
    #[serde(default = "default_memory_file_list_max_limit")]
    pub file_list_max_limit: usize,
}

fn default_memory_root_filename() -> String {
    "MEMORY.md".to_string()
}

fn default_memory_dirname() -> String {
    "memory".to_string()
}

fn default_memory_file_list_default_limit() -> usize {
    20
}

fn default_memory_file_list_max_limit() -> usize {
    90
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            root_memory_filename: default_memory_root_filename(),
            memory_dirname: default_memory_dirname(),
            file_list_default_limit: default_memory_file_list_default_limit(),
            file_list_max_limit: default_memory_file_list_max_limit(),
        }
    }
}
