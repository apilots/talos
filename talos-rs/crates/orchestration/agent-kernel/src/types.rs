use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use talos_error::{TalosError, TalosResult};
use talos_types::talos::{TalosOutputItem, TalosUsage};
use tokio::sync::mpsc;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum TurnPhase {
    #[default]
    Execution,
    Planning,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskKind {
    General,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TaskConstraints {
    pub timeout_ms: Option<u64>,
    pub max_tool_calls: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskSpec {
    pub id: String,
    pub kind: TaskKind,
    pub goal: String,
    pub input: Value,
    pub constraints: TaskConstraints,
    pub tool_candidates: Vec<String>,
    pub expected_output: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TaskPlan {
    pub tasks: Vec<TaskSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Success,
    Partial,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskResult {
    pub task_id: String,
    pub kind: TaskKind,
    pub goal: String,
    pub status: TaskStatus,
    pub output: String,
    #[serde(default)]
    pub output_items: Vec<TalosOutputItem>,
    #[serde(default)]
    pub usage: TalosUsage,
    pub tool_traces: Vec<Value>,
    pub state_delta: HashMap<String, Value>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KernelEventType {
    UserInput,
    PlanCreated,
    TaskCompleted,
    Transfer,
    FinalResponse,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KernelEvent {
    pub event_type: KernelEventType,
    pub message: String,
    pub data: Option<Value>,
    pub timestamp: i64,
}

impl KernelEvent {
    pub fn new(
        event_type: KernelEventType,
        message: impl Into<String>,
        data: Option<Value>,
    ) -> Self {
        Self {
            event_type,
            message: message.into(),
            data,
            timestamp: Utc::now().timestamp_millis(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KernelStreamEvent {
    pub event_type: String,
    pub data: Value,
    pub timestamp: i64,
}

impl KernelStreamEvent {
    pub fn new(event_type: impl Into<String>, data: Value) -> Self {
        Self {
            event_type: event_type.into(),
            data,
            timestamp: Utc::now().timestamp_millis(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvocationContext {
    pub session_id: String,
    pub invocation_id: String,
    pub agent_id: String,
    pub workspace_id: String,
    pub input: String,
    pub model: String,
    #[serde(default)]
    pub phase: TurnPhase,
    pub planning_instruction: Option<String>,
    pub transfer_to: Option<String>,
    pub state: HashMap<String, Value>,
    #[serde(default)]
    pub state_bags: ContextStateBags,
    #[serde(default)]
    pub limits: InvocationLimits,
    pub events: Vec<KernelEvent>,
    #[serde(skip)]
    pub stream_tx: Option<mpsc::Sender<KernelStreamEvent>>,
}

impl InvocationContext {
    pub fn set_session_state(&mut self, key: impl Into<String>, value: Value) {
        self.state_bags.session_state.insert(key.into(), value);
    }

    pub fn set_request_state(&mut self, key: impl Into<String>, value: Value) {
        self.state_bags.request_state.insert(key.into(), value);
    }

    pub fn set_user_state(&mut self, _key: impl Into<String>, _value: Value) -> TalosResult<()> {
        Err(TalosError::ValidationError(
            "user_state is read-only by default in InvocationContext".to_string(),
        ))
    }

    pub fn set_workspace_state(
        &mut self,
        _key: impl Into<String>,
        _value: Value,
    ) -> TalosResult<()> {
        Err(TalosError::ValidationError(
            "workspace_state is read-only by default in InvocationContext".to_string(),
        ))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRunResult {
    pub output_text: String,
    #[serde(default)]
    pub output_items: Vec<TalosOutputItem>,
    pub stop_reason: String,
    #[serde(default)]
    pub usage: TalosUsage,
    pub task_results: Vec<TaskResult>,
    pub state_delta: HashMap<String, Value>,
    pub continue_loop: bool,
    pub requested_transfer: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ContextStateBags {
    #[serde(default)]
    pub session_state: HashMap<String, Value>,
    #[serde(default)]
    pub user_state: HashMap<String, Value>,
    #[serde(default)]
    pub workspace_state: HashMap<String, Value>,
    #[serde(default)]
    pub request_state: HashMap<String, Value>,
}

/// Runtime limits for invocation execution.
/// This is a simplified version that directly stores limit values
/// extracted from AgentKernelConfig.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvocationLimits {
    pub max_iters: u32,
    pub max_task_per_turn: u32,
    pub tool_parallelism: usize,
    pub max_llm_calls: u32,
}

impl Default for InvocationLimits {
    fn default() -> Self {
        let config = talos_config::AgentKernelConfig::default();
        Self {
            max_iters: config.max_iters,
            max_task_per_turn: config.max_task_per_turn as u32,
            tool_parallelism: config.tool_parallelism,
            max_llm_calls: config.max_llm_calls,
        }
    }
}

impl From<&talos_config::AgentKernelConfig> for InvocationLimits {
    fn from(config: &talos_config::AgentKernelConfig) -> Self {
        Self {
            max_iters: config.max_iters,
            max_task_per_turn: config.max_task_per_turn as u32,
            tool_parallelism: config.tool_parallelism,
            max_llm_calls: config.max_llm_calls,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunRequest {
    pub session_id: String,
    pub invocation_id: String,
    pub agent_id: String,
    pub workspace_id: String,
    pub input: String,
    pub model: String,
    #[serde(default)]
    pub phase: TurnPhase,
    pub planning_instruction: Option<String>,
    pub transfer_to: Option<String>,
    pub initial_state_bags: Option<ContextStateBags>,
    pub kernel_config: Option<talos_config::AgentKernelConfig>,
    #[serde(skip)]
    pub stream_tx: Option<mpsc::Sender<KernelStreamEvent>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunResponse {
    pub output_text: String,
    #[serde(default)]
    pub output_items: Vec<TalosOutputItem>,
    pub stop_reason: String,
    #[serde(default)]
    pub usage: TalosUsage,
    pub model: String,
    pub task_results: Vec<TaskResult>,
    pub transfer_to: Option<String>,
    pub iterations: u32,
    pub events: Vec<KernelEvent>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_invocation_context_state_write_scopes() {
        let mut ctx = InvocationContext {
            session_id: "s1".to_string(),
            invocation_id: "i1".to_string(),
            agent_id: "a1".to_string(),
            workspace_id: "w1".to_string(),
            input: "hello".to_string(),
            model: "m1".to_string(),
            phase: TurnPhase::Execution,
            planning_instruction: None,
            transfer_to: None,
            state: HashMap::new(),
            state_bags: ContextStateBags::default(),
            limits: InvocationLimits::default(),
            events: Vec::new(),
            stream_tx: None,
        };

        ctx.set_session_state("k1", json!("v1"));
        ctx.set_request_state("k2", json!("v2"));

        assert_eq!(ctx.state_bags.session_state.get("k1"), Some(&json!("v1")));
        assert_eq!(ctx.state_bags.request_state.get("k2"), Some(&json!("v2")));
        assert!(ctx.set_user_state("k3", json!("v3")).is_err());
        assert!(ctx.set_workspace_state("k4", json!("v4")).is_err());
    }
}
