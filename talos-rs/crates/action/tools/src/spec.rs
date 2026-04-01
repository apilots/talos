use serde::{Deserialize, Serialize};
use talos_llm_provider::ToolDefinition;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToolSource {
    Local,
    Mcp,
    Dynamic,
    Collaboration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToolPayloadKind {
    Function,
    Custom,
    Resource,
    Mcp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToolParallelism {
    SerialOnly,
    ParallelSafe,
}

#[derive(Debug, Clone)]
pub struct RegisteredToolSpec {
    pub definition: ToolDefinition,
    pub source: ToolSource,
    pub payload_kind: ToolPayloadKind,
    pub parallelism: ToolParallelism,
    pub mutating: bool,
}

impl RegisteredToolSpec {
    pub fn new(
        definition: ToolDefinition,
        source: ToolSource,
        payload_kind: ToolPayloadKind,
        parallelism: ToolParallelism,
        mutating: bool,
    ) -> Self {
        Self {
            definition,
            source,
            payload_kind,
            parallelism,
            mutating,
        }
    }

    pub fn local_function(
        definition: ToolDefinition,
        parallelism: ToolParallelism,
        mutating: bool,
    ) -> Self {
        Self::new(
            definition,
            ToolSource::Local,
            ToolPayloadKind::Function,
            parallelism,
            mutating,
        )
    }
}

#[derive(Debug, Clone, Default)]
pub struct ToolCatalog {
    specs: Vec<RegisteredToolSpec>,
}

impl ToolCatalog {
    pub fn new(specs: Vec<RegisteredToolSpec>) -> Self {
        Self { specs }
    }

    pub fn specs(&self) -> &[RegisteredToolSpec] {
        &self.specs
    }

    pub fn model_definitions(&self) -> Vec<ToolDefinition> {
        self.specs
            .iter()
            .map(|spec| spec.definition.clone())
            .collect()
    }

    pub fn find(&self, name: &str) -> Option<&RegisteredToolSpec> {
        self.specs.iter().find(|spec| spec.definition.name == name)
    }

    pub fn supports_parallel(&self, name: &str) -> bool {
        self.find(name)
            .map(|spec| spec.parallelism == ToolParallelism::ParallelSafe)
            .unwrap_or(false)
    }
}
