/// User content kinds that an Agent Provider may accept beyond baseline text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AgentUserContentKind {
    SkillSelection,
    File,
    Audio,
    Video,
}

/// Typed UserAction kinds supported by one Agent Provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AgentUserActionKind {
    DynamicToolResult,
    Question,
    SandboxApproval,
}

/// Provider behavior that changes how AgentHarness admits an operation.
///
/// Streaming, text input, cancellation, event ordering, and cleanup are
/// baseline SPI requirements. Provider-private features such as MCP are not
/// declared here unless AgentHarness has a corresponding operation to gate.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AgentCapabilities {
    pub rich_input: Vec<AgentUserContentKind>,
    pub dynamic_input: bool,
    pub user_actions: Vec<AgentUserActionKind>,
    pub durable_continuation: bool,
    pub context_compaction: bool,
}
