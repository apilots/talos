use serde::{Deserialize, Serialize};
use talos_llm_provider::ToolDefinition;
use talos_tools_sandbox::{
    ToolApprovalRequirement, ToolGovernanceArgumentOverride, ToolGovernanceSpec, ToolSandboxMode,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToolSource {
    Local,
    Mcp,
    Dynamic,
    Collaboration,
    Skill,
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
    pub governance: ToolGovernanceSpec,
}

impl RegisteredToolSpec {
    pub fn new(
        definition: ToolDefinition,
        source: ToolSource,
        payload_kind: ToolPayloadKind,
        parallelism: ToolParallelism,
        mutating: bool,
        governance: ToolGovernanceSpec,
    ) -> Self {
        Self {
            definition,
            source,
            payload_kind,
            parallelism,
            mutating,
            governance,
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
            ToolGovernanceSpec::default(),
        )
    }

    pub fn with_governance(mut self, governance: ToolGovernanceSpec) -> Self {
        self.governance = governance;
        self
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

pub fn default_governance_for_source(source: ToolSource) -> ToolGovernanceSpec {
    match source {
        ToolSource::Collaboration | ToolSource::Skill => ToolGovernanceSpec {
            default_approval: ToolApprovalRequirement::NeedsApproval {
                reason: Some("tool spawns sub-agent work and requires approval".to_string()),
            },
            preferred_sandbox: ToolSandboxMode::UseDefault,
            argument_override: ToolGovernanceArgumentOverride::None,
            retry_without_sandbox: false,
        },
        ToolSource::Local | ToolSource::Mcp | ToolSource::Dynamic => ToolGovernanceSpec::default(),
    }
}
