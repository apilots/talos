use serde_json::Value;
use std::sync::Arc;
use talos_config::AgentKernelConfig;
use talos_llm_provider::LlmProvider;
use talos_memory::MemoryStore;
use talos_tools::ToolRegistry;
use tokio::sync::mpsc;

use crate::KernelStreamEvent;

/// Context for a general turn execution.
/// Encapsulates all parameters needed for run_general_turn.
pub struct GeneralTurnContext {
    pub session_id: String,
    pub agent_id: String,
    pub agent_name: String,
    pub agent_description: String,
    pub input: String,
    pub selected_model: String,
    pub llm_provider: Arc<dyn LlmProvider>,
    pub tool_registry: Arc<ToolRegistry>,
    pub memory_store: Arc<MemoryStore>,
    pub kernel_config: AgentKernelConfig,
    pub metadata: Option<Value>,
    pub stream_tx: Option<mpsc::Sender<KernelStreamEvent>>,
}

impl GeneralTurnContext {
    /// Create a new context builder.
    pub fn builder() -> GeneralTurnContextBuilder {
        GeneralTurnContextBuilder::new()
    }
}

/// Builder for GeneralTurnContext.
pub struct GeneralTurnContextBuilder {
    session_id: Option<String>,
    agent_id: Option<String>,
    agent_name: Option<String>,
    agent_description: Option<String>,
    input: Option<String>,
    selected_model: Option<String>,
    llm_provider: Option<Arc<dyn LlmProvider>>,
    tool_registry: Option<Arc<ToolRegistry>>,
    memory_store: Option<Arc<MemoryStore>>,
    kernel_config: Option<AgentKernelConfig>,
    metadata: Option<Value>,
    stream_tx: Option<mpsc::Sender<KernelStreamEvent>>,
}

impl GeneralTurnContextBuilder {
    pub fn new() -> Self {
        Self {
            session_id: None,
            agent_id: None,
            agent_name: None,
            agent_description: None,
            input: None,
            selected_model: None,
            llm_provider: None,
            tool_registry: None,
            memory_store: None,
            kernel_config: None,
            metadata: None,
            stream_tx: None,
        }
    }

    pub fn session_id(mut self, id: impl Into<String>) -> Self {
        self.session_id = Some(id.into());
        self
    }

    pub fn agent_id(mut self, id: impl Into<String>) -> Self {
        self.agent_id = Some(id.into());
        self
    }

    pub fn agent_name(mut self, name: impl Into<String>) -> Self {
        self.agent_name = Some(name.into());
        self
    }

    pub fn agent_description(mut self, desc: impl Into<String>) -> Self {
        self.agent_description = Some(desc.into());
        self
    }

    pub fn input(mut self, input: impl Into<String>) -> Self {
        self.input = Some(input.into());
        self
    }

    pub fn selected_model(mut self, model: impl Into<String>) -> Self {
        self.selected_model = Some(model.into());
        self
    }

    pub fn llm_provider(mut self, provider: Arc<dyn LlmProvider>) -> Self {
        self.llm_provider = Some(provider);
        self
    }

    pub fn tool_registry(mut self, registry: Arc<ToolRegistry>) -> Self {
        self.tool_registry = Some(registry);
        self
    }

    pub fn memory_store(mut self, store: Arc<MemoryStore>) -> Self {
        self.memory_store = Some(store);
        self
    }

    pub fn kernel_config(mut self, config: AgentKernelConfig) -> Self {
        self.kernel_config = Some(config);
        self
    }

    pub fn metadata(mut self, metadata: Option<Value>) -> Self {
        self.metadata = metadata;
        self
    }

    pub fn stream_tx(mut self, tx: Option<mpsc::Sender<KernelStreamEvent>>) -> Self {
        self.stream_tx = tx;
        self
    }

    pub fn build(self) -> GeneralTurnContext {
        GeneralTurnContext {
            session_id: self.session_id.expect("session_id is required"),
            agent_id: self.agent_id.expect("agent_id is required"),
            agent_name: self.agent_name.expect("agent_name is required"),
            agent_description: self
                .agent_description
                .expect("agent_description is required"),
            input: self.input.expect("input is required"),
            selected_model: self.selected_model.expect("selected_model is required"),
            llm_provider: self.llm_provider.expect("llm_provider is required"),
            tool_registry: self.tool_registry.expect("tool_registry is required"),
            memory_store: self.memory_store.expect("memory_store is required"),
            kernel_config: self.kernel_config.expect("kernel_config is required"),
            metadata: self.metadata,
            stream_tx: self.stream_tx,
        }
    }
}

impl Default for GeneralTurnContextBuilder {
    fn default() -> Self {
        Self::new()
    }
}
