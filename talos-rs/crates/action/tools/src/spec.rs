use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use talos_llm_provider::ToolDefinition;
use talos_tools_sandbox::{ToolGovernanceSpec, ToolSandboxOperationKind};

use crate::state::PermissionMode;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ToolSelectionPhase {
    Planning,
    #[default]
    Execution,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ToolVisibility {
    AlwaysLoad,
    #[default]
    DefaultVisible,
    Deferred,
    Disabled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ToolBehaviorFlags {
    pub read_only: bool,
    pub destructive: bool,
    pub concurrency_safe: bool,
    pub requires_user_interaction: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolResultPolicy {
    pub max_inline_bytes: usize,
    pub persist_large_output: bool,
    pub preview_lines: usize,
}

impl Default for ToolResultPolicy {
    fn default() -> Self {
        Self {
            max_inline_bytes: 64 * 1024,
            persist_large_output: false,
            preview_lines: 40,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RegisteredToolSpec {
    pub definition: ToolDefinition,
    pub source: ToolSource,
    pub payload_kind: ToolPayloadKind,
    pub parallelism: ToolParallelism,
    pub mutating: bool,
    pub governance: ToolGovernanceSpec,
    pub sandbox_operation: Option<ToolSandboxOperationKind>,
    pub internal_name: String,
    pub aliases: Vec<String>,
    pub visibility: ToolVisibility,
    pub behavior_flags: ToolBehaviorFlags,
    pub result_policy: ToolResultPolicy,
    pub user_facing_name: String,
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
        let behavior_flags = ToolBehaviorFlags {
            read_only: !mutating,
            destructive: mutating,
            concurrency_safe: parallelism == ToolParallelism::ParallelSafe && !mutating,
            requires_user_interaction: false,
        };
        Self {
            internal_name: definition.name.clone(),
            user_facing_name: definition.name.clone(),
            aliases: Vec::new(),
            visibility: ToolVisibility::DefaultVisible,
            result_policy: ToolResultPolicy::default(),
            definition,
            source,
            payload_kind,
            parallelism,
            mutating,
            governance,
            sandbox_operation: None,
            behavior_flags,
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

    pub fn with_sandbox_operation(mut self, operation: ToolSandboxOperationKind) -> Self {
        self.sandbox_operation = Some(operation);
        self
    }

    pub fn with_optional_sandbox_operation(
        mut self,
        operation: Option<ToolSandboxOperationKind>,
    ) -> Self {
        self.sandbox_operation = operation;
        self
    }

    pub fn with_internal_name(mut self, internal_name: impl Into<String>) -> Self {
        self.internal_name = internal_name.into();
        self
    }

    pub fn with_aliases<I, S>(mut self, aliases: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.aliases = aliases.into_iter().map(Into::into).collect();
        self
    }

    pub fn with_visibility(mut self, visibility: ToolVisibility) -> Self {
        self.visibility = visibility;
        self
    }

    pub fn with_behavior_flags(mut self, behavior_flags: ToolBehaviorFlags) -> Self {
        self.parallelism = if behavior_flags.concurrency_safe {
            ToolParallelism::ParallelSafe
        } else {
            ToolParallelism::SerialOnly
        };
        self.mutating = !behavior_flags.read_only;
        self.behavior_flags = behavior_flags;
        self
    }

    pub fn with_result_policy(mut self, result_policy: ToolResultPolicy) -> Self {
        self.result_policy = result_policy;
        self
    }

    pub fn with_user_facing_name(mut self, name: impl Into<String>) -> Self {
        self.user_facing_name = name.into();
        self
    }

    pub fn matches_name(&self, name: &str) -> bool {
        self.definition.name == name
            || self.internal_name == name
            || self.aliases.iter().any(|alias| alias == name)
    }

    pub fn is_visible_to_model(&self) -> bool {
        !matches!(
            self.visibility,
            ToolVisibility::Deferred | ToolVisibility::Disabled
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolSelectionRequest {
    pub phase: ToolSelectionPhase,
    pub planner_mode: Option<String>,
    pub permission_mode: PermissionMode,
    pub user_input: Option<String>,
    pub session_id: Option<String>,
}

impl Default for ToolSelectionRequest {
    fn default() -> Self {
        Self {
            phase: ToolSelectionPhase::Execution,
            planner_mode: None,
            permission_mode: PermissionMode::Default,
            user_input: None,
            session_id: None,
        }
    }
}

impl ToolSelectionRequest {
    pub fn with_user_input(mut self, user_input: impl Into<String>) -> Self {
        self.user_input = Some(user_input.into());
        self
    }

    pub fn with_session_id(mut self, session_id: impl Into<String>) -> Self {
        self.session_id = Some(session_id.into());
        self
    }
}

#[derive(Debug, Clone, Default)]
pub struct ToolSelection {
    active: Vec<RegisteredToolSpec>,
    deferred: Vec<RegisteredToolSpec>,
    always_loaded: Vec<RegisteredToolSpec>,
}

impl ToolSelection {
    pub fn new(
        active: Vec<RegisteredToolSpec>,
        deferred: Vec<RegisteredToolSpec>,
        always_loaded: Vec<RegisteredToolSpec>,
    ) -> Self {
        Self {
            active,
            deferred,
            always_loaded,
        }
    }

    pub fn active_specs(&self) -> &[RegisteredToolSpec] {
        &self.active
    }

    pub fn deferred_specs(&self) -> &[RegisteredToolSpec] {
        &self.deferred
    }

    pub fn always_loaded_specs(&self) -> &[RegisteredToolSpec] {
        &self.always_loaded
    }

    pub fn model_definitions(&self) -> Vec<ToolDefinition> {
        self.active
            .iter()
            .chain(self.always_loaded.iter())
            .map(|spec| spec.definition.clone())
            .collect()
    }

    pub fn visible_specs(&self) -> Vec<&RegisteredToolSpec> {
        let mut names = BTreeSet::new();
        let mut items = Vec::new();
        for spec in self.active.iter().chain(self.always_loaded.iter()) {
            if names.insert(spec.definition.name.clone()) {
                items.push(spec);
            }
        }
        items
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
            .filter(|spec| spec.is_visible_to_model())
            .map(|spec| spec.definition.clone())
            .collect()
    }

    pub fn find(&self, name: &str) -> Option<&RegisteredToolSpec> {
        self.specs.iter().find(|spec| spec.matches_name(name))
    }

    pub fn supports_parallel(&self, name: &str) -> bool {
        self.find(name)
            .map(|spec| spec.behavior_flags.concurrency_safe)
            .unwrap_or(false)
    }
}

pub fn default_governance_for_source(source: ToolSource) -> ToolGovernanceSpec {
    match source {
        ToolSource::Collaboration => ToolGovernanceSpec::default(),
        ToolSource::Local | ToolSource::Mcp | ToolSource::Dynamic => ToolGovernanceSpec::default(),
    }
}
