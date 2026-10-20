//! Talos Common Types
//!
//! Defines common types and ID structures.

pub use talos_error::{TalosError, TalosResult};

pub mod agent_input;
pub mod auth_api;
pub mod browser;
pub mod config_api;
pub mod event_api;
pub mod file_api;
pub mod fs;
pub mod ids;
pub mod mcp;
pub mod mcp_credentials;
pub mod message_queue;
pub mod model;
pub mod node;
pub mod plugin;
pub mod profile_api;
mod response;
pub mod runtime;
pub mod schema;
pub mod session;
pub mod skills;
pub mod subagent;
pub mod talos;
mod talos_content_display;
pub mod turn_api;
pub mod usage;
pub mod user_action;

pub use agent_input::*;
pub use auth_api::*;
pub use browser::*;
pub use config_api::*;
pub use event_api::*;
pub use file_api::*;
pub use fs::*;
pub use ids::*;
pub use mcp::*;
pub use mcp_credentials::*;
pub use message_queue::*;
pub use model::*;
pub use node::*;
pub use plugin::*;
pub use profile_api::*;
pub use response::ApiResponse;
pub use runtime::*;
pub use session::*;
pub use skills::*;
pub use subagent::*;
pub use talos::*;
pub use turn_api::*;
pub use usage::*;
pub use user_action::*;

#[cfg(test)]
#[path = "types_tests.rs"]
mod tests;

mod desktop;
pub use desktop::*;

mod timeline_cursor;
pub use timeline_cursor::TimelineEventId;

mod session_execution;
pub use session_execution::SessionHistoryCursor;

mod sync;
mod sync_metrics;
pub use sync::{SyncConnectionState, SyncEnableRequest, SyncStatusResponse};
pub use sync_metrics::{SyncMetadataCounts, SyncMetricsResponse, SyncPeerMetrics};

mod browser_action;
pub use browser_action::*;
mod browser_host;
pub use browser_host::*;

mod tool_file;
pub use tool_file::{ToolInputFile, ToolOutputFile};
