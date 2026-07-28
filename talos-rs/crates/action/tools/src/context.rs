use dashmap::DashMap;
use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;
use talos_config::AgentKernelConfig;
use talos_memory::{MemoryReader, MemoryScope};

use crate::handlers::dynamic::StoredDynamicToolResult;
use crate::state::{
    get_session_tool_state, CatalogToolEntry, ContentReplacementState, DeferredToolState,
    PermissionMode, ReadFileStateStore, SessionToolState, TaskStateEntry, TaskStateStore,
    ToolCatalogState, ToolPermissionContext, ToolResultBudgetState,
};
use crate::AgentControl;

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
    kernel_config: Option<Arc<AgentKernelConfig>>,
    agent_control: Option<Arc<dyn AgentControl>>,
    memory_reader: Option<Arc<dyn MemoryReader>>,
    memory_scope: Option<MemoryScope>,
    session_state: Arc<SessionToolState>,
    progress_emitter: Option<ToolProgressEmitter>,
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
        Self {
            session_id: session_id.clone(),
            turn_id: None,
            agent_id: None,
            user_id: user_id.into(),
            project_root: None,
            dynamic_tool_results: Arc::new(DashMap::new()),
            kernel_config,
            agent_control: None,
            memory_reader: None,
            memory_scope: None,
            session_state: get_session_tool_state(&session_id),
            progress_emitter: None,
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

    pub fn permission_context(&self) -> &ToolPermissionContext {
        &self.session_state.permissions
    }

    pub fn permission_mode(&self) -> PermissionMode {
        self.permission_context().mode()
    }

    pub fn set_permission_mode(&self, mode: PermissionMode) {
        self.permission_context().set_mode(mode);
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
}

impl Default for ToolContext {
    fn default() -> Self {
        Self::anonymous()
    }
}

#[cfg(test)]
#[path = "context_tests.rs"]
mod tests;
