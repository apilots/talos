use dashmap::DashMap;
use futures_util::future::{BoxFuture, FutureExt, Shared};
use std::fmt;
use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;
use talos_config::AgentKernelConfig;
use talos_memory::{MemoryReader, MemoryScope, UserWorkspaceMemoryWriter};
use talos_tools_sandbox::{ToolFileSystemHandle, ToolSandboxEnvironment, ToolSandboxPermit};
use talos_types::{UserActionResponse, UserPermissionMode};

use crate::handlers::dynamic::StoredDynamicToolResult;
use crate::state::{
    get_session_tool_state, CatalogToolEntry, ContentReplacementState, DeferredToolState,
    ReadFileStateStore, SessionToolState, TaskStateEntry, TaskStateStore, ToolCatalogState,
    ToolResultBudgetState,
};
use crate::AgentControl;
use crate::CommandRiskAssessor;

/// Classifies best-effort progress emitted while a tool is still running.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolProgressKind {
    Status,
    Stdout,
    Stderr,
}

/// A best-effort progress event produced by a tool runtime before final output is available.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolProgressEvent {
    pub kind: ToolProgressKind,
    pub text: String,
}

/// Emits tool progress into the owning runtime without coupling tools to a transport.
#[derive(Clone)]
pub struct ToolProgressEmitter {
    sink: Arc<dyn Fn(ToolProgressEvent) + Send + Sync>,
}

/// Narrow Run cancellation facade projected by Agent Kernel.
#[derive(Clone)]
pub struct InvocationCancellation {
    requested: Shared<BoxFuture<'static, ()>>,
}

impl InvocationCancellation {
    /// Build a Tool projection from an orchestration-owned cancellation signal.
    pub fn new(requested: impl Future<Output = ()> + Send + 'static) -> Self {
        Self {
            requested: requested.boxed().shared(),
        }
    }

    /// Wait until cancellation is requested for the owning Run.
    pub async fn requested(&self) {
        self.requested.clone().await;
    }
}

impl ToolProgressEmitter {
    pub fn new(sink: impl Fn(ToolProgressEvent) + Send + Sync + 'static) -> Self {
        Self {
            sink: Arc::new(sink),
        }
    }

    pub fn emit(&self, event: ToolProgressEvent) {
        (self.sink)(event);
    }
}

impl fmt::Debug for ToolProgressEmitter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ToolProgressEmitter")
    }
}

/// Execution context passed to tools.
#[derive(Clone)]
pub struct ToolContext {
    session_id: String,
    turn_id: Option<String>,
    agent_id: Option<String>,
    user_id: String,
    project_root: Option<PathBuf>,
    dynamic_tool_results: Arc<DashMap<String, StoredDynamicToolResult>>,
    user_action_responses: Arc<DashMap<String, UserActionResponse>>,
    sandbox_approval_fingerprints: Arc<DashMap<String, String>>,
    current_call_id: Option<String>,
    kernel_config: Option<Arc<AgentKernelConfig>>,
    agent_control: Option<Arc<dyn AgentControl>>,
    memory_reader: Option<Arc<dyn MemoryReader>>,
    user_workspace_memory_writer: Option<Arc<dyn UserWorkspaceMemoryWriter>>,
    memory_scope: Option<MemoryScope>,
    session_state: Arc<SessionToolState>,
    progress_emitter: Option<ToolProgressEmitter>,
    sandbox_user_permission_mode: UserPermissionMode,
    sandbox_environment: Option<ToolSandboxEnvironment>,
    sandbox_permit: Option<ToolSandboxPermit>,
    command_risk_assessor: Option<Arc<dyn CommandRiskAssessor>>,
    cancellation: Option<InvocationCancellation>,
}

impl ToolContext {
    pub fn new(session_id: impl Into<String>, user_id: impl Into<String>) -> Self {
        Self::new_with_kernel_config(session_id, user_id, None)
    }

    pub fn new_with_kernel_config(
        session_id: impl Into<String>,
        user_id: impl Into<String>,
        kernel_config: Option<Arc<AgentKernelConfig>>,
    ) -> Self {
        let session_id = session_id.into();
        let sandbox_user_permission_mode = kernel_config
            .as_ref()
            .map(|config| {
                UserPermissionMode::from(&config.tools.sandbox_policy.user_permission_mode)
            })
            .unwrap_or_default();
        Self {
            session_id: session_id.clone(),
            turn_id: None,
            agent_id: None,
            user_id: user_id.into(),
            project_root: None,
            dynamic_tool_results: Arc::new(DashMap::new()),
            user_action_responses: Arc::new(DashMap::new()),
            sandbox_approval_fingerprints: Arc::new(DashMap::new()),
            current_call_id: None,
            kernel_config,
            agent_control: None,
            memory_reader: None,
            user_workspace_memory_writer: None,
            memory_scope: None,
            session_state: get_session_tool_state(&session_id),
            progress_emitter: None,
            sandbox_user_permission_mode,
            sandbox_environment: None,
            sandbox_permit: None,
            command_risk_assessor: None,
            cancellation: None,
        }
    }

    /// Attaches the application-owned child Session control port.
    pub fn with_agent_control(mut self, control: Arc<dyn AgentControl>) -> Self {
        self.agent_control = Some(control);
        self
    }

    pub fn with_turn_id(mut self, turn_id: impl Into<String>) -> Self {
        self.turn_id = Some(turn_id.into());
        self
    }

    /// Attaches the trusted identity of the Agent executing this Tool.
    pub fn with_agent_id(mut self, agent_id: impl Into<String>) -> Self {
        self.agent_id = Some(agent_id.into());
        self
    }

    pub fn with_memory_reader(mut self, reader: Arc<dyn MemoryReader>) -> Self {
        self.memory_reader = Some(reader);
        self
    }

    /// Attaches the application-owned User Workspace Memory mutation capability.
    pub fn with_user_workspace_memory_writer(
        mut self,
        writer: Arc<dyn UserWorkspaceMemoryWriter>,
    ) -> Self {
        self.user_workspace_memory_writer = Some(writer);
        self
    }

    pub fn with_memory_scope(mut self, scope: MemoryScope) -> Self {
        self.memory_scope = Some(scope);
        self
    }

    pub fn with_project_root(mut self, project_root: impl Into<PathBuf>) -> Self {
        self.project_root = Some(project_root.into());
        self
    }

    pub fn with_progress_emitter(mut self, emitter: ToolProgressEmitter) -> Self {
        self.progress_emitter = Some(emitter);
        self
    }

    /// Overrides the configured sandbox permission mode for this tool invocation context.
    pub fn with_sandbox_user_permission_mode(mut self, mode: UserPermissionMode) -> Self {
        self.sandbox_user_permission_mode = mode;
        self
    }

    /// Attaches the Runtime-resolved filesystem and execution boundary for Sandbox policy.
    pub fn with_sandbox_environment(mut self, environment: ToolSandboxEnvironment) -> Self {
        self.sandbox_environment = Some(environment);
        self
    }

    /// Returns the Runtime-resolved Sandbox boundary without deriving paths locally.
    pub fn sandbox_environment(&self) -> Option<&ToolSandboxEnvironment> {
        self.sandbox_environment.as_ref()
    }

    pub fn with_command_risk_assessor(mut self, assessor: Arc<dyn CommandRiskAssessor>) -> Self {
        self.command_risk_assessor = Some(assessor);
        self
    }

    /// Attach cancellation scoped to the owning Agent Run.
    pub fn with_cancellation(mut self, cancellation: InvocationCancellation) -> Self {
        self.cancellation = Some(cancellation);
        self
    }

    /// Return cancellation scoped to the owning Agent Run when available.
    pub fn cancellation(&self) -> Option<InvocationCancellation> {
        self.cancellation.clone()
    }

    pub fn command_risk_assessor(&self) -> Option<Arc<dyn CommandRiskAssessor>> {
        self.command_risk_assessor.clone()
    }

    pub fn with_catalog_snapshot(self, entries: Vec<CatalogToolEntry>) -> Self {
        self.tool_catalog_state().replace(entries);
        self
    }

    pub fn anonymous() -> Self {
        Self::new("anonymous-session", "anonymous-user")
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub fn user_id(&self) -> &str {
        &self.user_id
    }

    pub fn turn_id(&self) -> Option<&str> {
        self.turn_id.as_deref()
    }

    /// Returns the trusted executing Agent identity when this is an Agent Tool call.
    pub fn agent_id(&self) -> Option<&str> {
        self.agent_id.as_deref()
    }

    pub fn kernel_config(&self) -> Option<&AgentKernelConfig> {
        self.kernel_config.as_deref()
    }

    /// Returns the application-owned child Session control port when configured.
    pub fn agent_control(&self) -> Option<Arc<dyn AgentControl>> {
        self.agent_control.clone()
    }

    pub fn memory_reader(&self) -> Option<Arc<dyn MemoryReader>> {
        self.memory_reader.clone()
    }

    /// Returns the scoped User Workspace Memory mutation capability.
    pub fn user_workspace_memory_writer(&self) -> Option<Arc<dyn UserWorkspaceMemoryWriter>> {
        self.user_workspace_memory_writer.clone()
    }

    pub fn memory_scope(&self) -> Option<&MemoryScope> {
        self.memory_scope.as_ref()
    }

    pub fn project_root(&self) -> Option<PathBuf> {
        self.project_root.clone()
    }

    pub fn set_dynamic_tool_result(
        &self,
        call_id: impl Into<String>,
        result: StoredDynamicToolResult,
    ) {
        self.dynamic_tool_results.insert(call_id.into(), result);
    }

    pub fn get_dynamic_tool_result(&self, call_id: &str) -> Option<StoredDynamicToolResult> {
        self.dynamic_tool_results
            .get(call_id)
            .map(|result| result.value().clone())
    }

    pub fn set_user_action_response(
        &self,
        call_id: impl Into<String>,
        response: UserActionResponse,
    ) {
        self.user_action_responses.insert(call_id.into(), response);
    }

    pub fn get_user_action_response(&self, call_id: &str) -> Option<UserActionResponse> {
        self.user_action_responses
            .get(call_id)
            .map(|response| response.value().clone())
    }

    pub fn set_sandbox_approval_fingerprint(
        &self,
        call_id: impl Into<String>,
        fingerprint: impl Into<String>,
    ) {
        self.sandbox_approval_fingerprints
            .insert(call_id.into(), fingerprint.into());
    }

    pub fn get_sandbox_approval_fingerprint(&self, call_id: &str) -> Option<String> {
        self.sandbox_approval_fingerprints
            .get(call_id)
            .map(|fingerprint| fingerprint.value().clone())
    }

    pub fn current_call_id(&self) -> Option<&str> {
        self.current_call_id.as_deref()
    }

    pub(crate) fn bind_current_call_id(&mut self, call_id: impl Into<String>) {
        self.current_call_id = Some(call_id.into());
    }

    pub fn read_file_state(&self) -> &ReadFileStateStore {
        &self.session_state.read_files
    }

    pub fn task_state_store(&self) -> &TaskStateStore {
        &self.session_state.tasks
    }

    pub fn deferred_tool_state(&self) -> &DeferredToolState {
        &self.session_state.deferred_tools
    }

    pub fn result_budget_state(&self) -> &ToolResultBudgetState {
        &self.session_state.result_budget
    }

    pub fn tool_catalog_state(&self) -> &ToolCatalogState {
        &self.session_state.catalog
    }

    pub fn content_replacement_state(&self) -> &ContentReplacementState {
        &self.session_state.content_replacement
    }

    pub fn sandbox_user_permission_mode(&self) -> UserPermissionMode {
        self.sandbox_user_permission_mode
    }

    pub fn replace_tasks(&self, tasks: Vec<TaskStateEntry>) {
        self.task_state_store().replace(tasks);
    }

    /// Emits a best-effort progress event for interactive clients.
    pub fn emit_progress(&self, kind: ToolProgressKind, text: impl Into<String>) {
        if let Some(emitter) = &self.progress_emitter {
            emitter.emit(ToolProgressEvent {
                kind,
                text: text.into(),
            });
        }
    }

    /// Returns the optional progress emitter attached by the execution runtime.
    pub fn progress_emitter(&self) -> Option<ToolProgressEmitter> {
        self.progress_emitter.clone()
    }

    pub(crate) fn bind_sandbox_permit(&mut self, permit: ToolSandboxPermit) {
        self.sandbox_permit = Some(permit);
    }

    pub(crate) fn sandbox_permit(&self) -> talos_error::TalosResult<&ToolSandboxPermit> {
        self.sandbox_permit.as_ref().ok_or_else(|| {
            talos_error::TalosError::internal(
                "tool runtime requires an invocation-bound sandbox permit",
            )
        })
    }

    pub(crate) fn sandbox_file_system(&self) -> talos_error::TalosResult<ToolFileSystemHandle> {
        self.sandbox_permit()?.file_system().map_err(|error| {
            talos_error::TalosError::internal_with_source(
                "failed to bind invocation sandbox filesystem; technical details were withheld.",
                error,
            )
        })
    }
}

impl Default for ToolContext {
    fn default() -> Self {
        Self::anonymous()
    }
}

#[cfg(test)]
#[path = "context_tests.rs"]
mod tests;
