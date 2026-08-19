use anyhow::{anyhow, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Scenario {
    Smoke,
    Malformed,
    Replay,
    MultiSubscriber,
    TurnSmoke,
    ActiveTurnConflict,
    ResourceMatrix,
    CloudSessionCache,
    SkillsE2e,
    McpE2e,
    MessageQueueE2e,
}

impl Scenario {
    pub(crate) fn parse(value: Option<&str>) -> Result<Self> {
        match value.unwrap_or("smoke") {
            "smoke" => Ok(Self::Smoke),
            "malformed" => Ok(Self::Malformed),
            "replay" => Ok(Self::Replay),
            "multi-subscriber" => Ok(Self::MultiSubscriber),
            "turn-smoke" => Ok(Self::TurnSmoke),
            "active-turn-conflict" => Ok(Self::ActiveTurnConflict),
            "resource-matrix" => Ok(Self::ResourceMatrix),
            "cloud-session-cache" => Ok(Self::CloudSessionCache),
            "skills-e2e" => Ok(Self::SkillsE2e),
            "mcp-e2e" => Ok(Self::McpE2e),
            "message-queue-e2e" => Ok(Self::MessageQueueE2e),
            other => Err(anyhow!("unsupported scenario: {other}")),
        }
    }

    pub(crate) fn uses_cloud_session_cache(self) -> bool {
        matches!(self, Self::CloudSessionCache)
    }
}
