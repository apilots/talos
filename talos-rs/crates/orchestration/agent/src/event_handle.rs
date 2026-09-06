/// Transient Provider capability for one canonical message lifecycle.
#[derive(Debug, Clone)]
pub struct AgentMessageHandle {
    pub(crate) sink_id: String,
    pub(crate) message_id: String,
}

impl AgentMessageHandle {
    /// Return the canonical message id allocated by this sink.
    pub fn message_id(&self) -> &str {
        &self.message_id
    }
}

/// Transient Provider capability for one canonical block lifecycle.
#[derive(Debug, Clone)]
pub struct AgentMessageBlockHandle {
    pub(crate) sink_id: String,
    pub(crate) message_id: String,
    pub(crate) block_id: String,
}

impl AgentMessageBlockHandle {
    /// Return the canonical parent message id for this block.
    pub fn message_id(&self) -> &str {
        &self.message_id
    }

    /// Return the canonical block id allocated by this sink.
    pub fn block_id(&self) -> &str {
        &self.block_id
    }
}
