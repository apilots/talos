use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use talos_error::TalosResult;
use talos_types::canonical::{CanonicalOutputItem, CanonicalUsage};
use tokio::sync::mpsc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestratorRequest {
    pub agent_id: String,
    pub session_id: String,
    pub invocation_id: Option<String>,
    pub workspace_id: String,
    pub input: String,
    pub model: Option<String>,
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestratorExecution {
    pub output_text: String,
    #[serde(default)]
    pub output_items: Vec<CanonicalOutputItem>,
    pub stop_reason: String,
    #[serde(default)]
    pub usage: CanonicalUsage,
    pub task_count: u32,
    pub metadata: HashMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestratorEvent {
    pub event_type: String,
    pub data: Value,
}

#[async_trait]
pub trait AgentOrchestrator: Send + Sync {
    fn id(&self) -> &str;
    async fn run(&self, request: OrchestratorRequest) -> TalosResult<OrchestratorExecution>;
    async fn run_stream(
        &self,
        request: OrchestratorRequest,
    ) -> TalosResult<mpsc::Receiver<OrchestratorEvent>>;
}
