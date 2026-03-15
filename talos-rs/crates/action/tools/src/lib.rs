pub mod context;
pub mod registry;
pub mod r#trait;
pub mod trait_impl;

pub use context::ToolContext;
pub use r#trait::{Tool, ToolOutput};
pub use registry::ToolRegistry;

// Built-in tools
pub mod tools {
    pub mod edit;
    pub mod exec;
    pub mod network;
    pub mod read;
    pub mod write;
}

// Re-export the built-in tools
pub use tools::{
    edit::EditTool,
    exec::ExecTool,
    network::{HttpRequestTool, WebFetchTool, WebSearchTool},
    read::ReadTool,
    write::WriteTool,
};
