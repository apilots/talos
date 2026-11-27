mod access;
#[cfg(feature = "product")]
mod account;
#[cfg(feature = "product")]
mod agent_node;
#[cfg(feature = "product")]
pub mod automation;
mod config;
#[cfg(feature = "product")]
mod desktop;
pub mod file;
mod fs;
#[cfg(feature = "product")]
mod history;
#[cfg(feature = "product")]
mod inner_model;
#[cfg(feature = "product")]
mod keyed_gates;
mod memory;
#[cfg(feature = "product")]
mod message_queue;
#[cfg(feature = "product")]
mod model;
#[cfg(feature = "product")]
mod native_jobs;
mod plugin;
#[cfg(feature = "product")]
mod profile;
#[cfg(feature = "product")]
mod project;
#[cfg(feature = "product")]
mod service;
#[cfg(feature = "product")]
mod session;
mod skills;
#[cfg(feature = "product")]
mod subagent;
#[cfg(feature = "product")]
mod timeline;
#[cfg(feature = "product")]
pub mod turn;
mod user_workspace_memory;
mod workspace;
mod workspace_docs;

#[cfg(all(test, feature = "product"))]
mod test_support;

pub use access::Actor;
#[cfg(feature = "product")]
pub use account::AccountService;
#[cfg(feature = "control-plane")]
pub use agent_node::{AgentNodeRevocationHook, AgentNodeService, NodePrincipal};
#[cfg(feature = "product")]
pub use agent_node::{
    CreateNodeRequest, GetNodeRequest, ListNodesRequest, ProductNodeAccess,
    RotateCredentialRequest, UpdateNodeRequest,
};
pub use config::{ConfigReloadHook, ConfigReloadPlan, ConfigReloadTrigger, ConfigService};
#[cfg(feature = "product")]
pub use desktop::DesktopIngress;
pub use file::{FileByteStream, FileContent, FileService};
#[cfg(feature = "product")]
pub use history::SessionHistoryService;
#[cfg(feature = "product")]
pub use inner_model::{
    resolve_inner_model_ref, InnerModelChatParams, InnerModelClient, InnerModelClientBuilder,
};
pub use memory::MemoryService;
#[cfg(feature = "product")]
pub use message_queue::SessionMessageQueueService;
#[cfg(feature = "product")]
pub use native_jobs::{
    NativeJobFuture, NativeJobHost, NativeJobPauseUpdate, NativeJobRuntime, ResolvedNativeJobs,
};
pub use plugin::{mcp_relay_callback_uri, McpCredentialStore, PluginManagerService};
#[cfg(feature = "product")]
pub use profile::{ProfileService, ProfileUsageRetention};
#[cfg(feature = "product")]
pub use service::{McpPersonalCallback, ServiceResult, TalosService};
pub use skills::SkillsService;
#[cfg(feature = "product")]
pub use timeline::{
    TimelineCursorStatus, TimelineEventService, TimelineEventStream, TimelineSubscribeCommand,
    TimelineSubscription, TimelineSubscriptionCloseReason, TimelineSubscriptionClosed,
    TimelineSubscriptionMessage, TimelineSubscriptionService,
};
pub use user_workspace_memory::UserWorkspaceMemoryService;
pub use workspace::{EffectiveUserWorkspace, EffectiveWorkspaceResolver};
pub use workspace_docs::WorkspaceDocService;

mod worker_environment;
pub use worker_environment::WorkerEnvironmentService;
#[cfg(feature = "control-plane")]
mod worker_runtime_settings;
#[cfg(feature = "control-plane")]
pub use worker_runtime_settings::{
    connect_docker_worker_runtime, connect_kubernetes_worker_runtime,
};

#[cfg(feature = "product")]
pub use service::ManagementHost;

#[cfg(feature = "product")]
pub use agent_node::ComputerManagement;

#[cfg(all(test, feature = "product"))]
mod test_worker;

mod worker_trust;
pub use worker_trust::read_worker_trusted_ca;
#[cfg(all(feature = "product", not(feature = "control-plane")))]
#[path = "worker_runtime_unavailable.rs"]
mod worker_runtime_unavailable;
