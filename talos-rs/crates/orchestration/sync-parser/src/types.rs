use serde::{Deserialize, Serialize};
use talos_agent_api::{ActiveTurnState, TimelineEventPayload};
use talos_types::{
    AgentTurnRef, FileInfo, IdempotentCommand, NodeInfo, ProjectInfo, SessionConf, SessionInfo,
    SessionTreeIdentity, TalosSessionEnvironment, TurnCommandStatus,
};

use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_sync_types::{
    SyncResourceKey as WireKey, SyncResourceSummary as WireSummary, SyncScope, SyncVersion,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ResourceKind {
    Computer,
    Project,
    Session,
    File,
}

impl ResourceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Computer => "computer",
            Self::Project => "project",
            Self::Session => "session",
            Self::File => "file",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum SyncResourceKey {
    Computer { computer_id: String },
    Project { project_id: String },
    Session { session_id: String },
    File { file_id: String },
}

impl SyncResourceKey {
    pub fn kind(&self) -> ResourceKind {
        match self {
            Self::Computer { .. } => ResourceKind::Computer,
            Self::Project { .. } => ResourceKind::Project,
            Self::Session { .. } => ResourceKind::Session,
            Self::File { .. } => ResourceKind::File,
        }
    }

    pub fn id(&self) -> &str {
        match self {
            Self::Computer { computer_id } => computer_id,
            Self::Project { project_id } => project_id,
            Self::Session { session_id } => session_id,
            Self::File { file_id } => file_id,
        }
    }

    pub fn wire(&self) -> WireKey {
        WireKey::new(self.kind().as_str(), self.id())
    }

    pub fn validate(&self) -> talos_sync_types::SyncResult<()> {
        self.wire().validate()
    }
}

/// Translate recognized Talos namespaces. An account feed may carry other
/// application documents alongside Talos documents on the same Hub.
pub fn from_wire_key(key: WireKey) -> TalosResult<Option<SyncResourceKey>> {
    key.validate().map_err(crate::sync_error)?;
    Ok(match key.namespace.as_str() {
        "computer" => Some(SyncResourceKey::Computer {
            computer_id: key.id,
        }),
        "project" => Some(SyncResourceKey::Project { project_id: key.id }),
        "session" => Some(SyncResourceKey::Session { session_id: key.id }),
        "file" => Some(SyncResourceKey::File { file_id: key.id }),
        _ => None,
    })
}

impl TryFrom<WireKey> for SyncResourceKey {
    type Error = TalosError;

    fn try_from(key: WireKey) -> TalosResult<Self> {
        from_wire_key(key)?.ok_or_else(|| {
            TalosError::new(
                ErrorCode::InvalidRequest,
                "Unknown Talos synchronization namespace.",
            )
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyncResourceSummary {
    pub resource: SyncResourceKey,
    pub computer_id: Option<String>,
    pub version: SyncVersion,
    pub deleted: bool,
}

impl TryFrom<WireSummary> for SyncResourceSummary {
    type Error = TalosError;

    fn try_from(summary: WireSummary) -> TalosResult<Self> {
        Ok(Self {
            resource: summary.resource.try_into()?,
            computer_id: summary
                .related
                .filter(|key| key.namespace == ResourceKind::Computer.as_str())
                .map(|key| key.id),
            version: summary.version,
            deleted: summary.deleted,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ComputerAlias {
    pub source_node_id: String,
    pub computer_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SyncBinding {
    pub scope: SyncScope,
    pub peer_id: String,
    pub source_user_id: String,
    pub computers: Vec<ComputerAlias>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionExecutionBinding {
    pub computer_id: String,
    pub agent_id: String,
    pub workspace_id: String,
    pub workspace_user_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SyncEventKey {
    Agent {
        identity: AgentTurnRef,
        sequence: u64,
    },
    Notice {
        peer_id: String,
        source_event_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncSessionSnapshot {
    pub session: SessionInfo,
    pub conf: SessionConf,
    pub environment: Option<TalosSessionEnvironment>,
    pub channel_id: Option<String>,
    pub thread_id: Option<String>,
    pub binding: SessionExecutionBinding,
    pub tree_identity: SessionTreeIdentity,
    pub memory_processed_through: Option<SyncEventKey>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncFileSnapshot {
    pub file: FileInfo,
    pub storage_peer_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum SyncResourceSnapshot {
    Computer(NodeInfo),
    Project(ProjectInfo),
    Session(Box<SyncSessionSnapshot>),
    File(SyncFileSnapshot),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncTurnMetadata {
    pub identity: AgentTurnRef,
    pub coordinator_id: String,
    pub previous_turn_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncTimelineEntry {
    pub key: SyncEventKey,
    pub event: TimelineEventPayload,
    pub persisted_at: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncActiveTurnMetadata {
    pub identity: AgentTurnRef,
    pub state: ActiveTurnState,
    pub updated_at: i64,
    pub last_event: SyncEventKey,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncAcceptedCommand {
    pub command: IdempotentCommand,
    pub idempotency_key: String,
    pub request_id: String,
    pub request_hash: String,
    pub identity: AgentTurnRef,
    pub message_id: String,
    pub status: TurnCommandStatus,
    pub committed_event: SyncEventKey,
    pub expires_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    content = "data",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SyncChange {
    Metadata {
        before: Option<Box<SyncResourceSnapshot>>,
        after: Box<SyncResourceSnapshot>,
    },
    Turn(SyncTurnMetadata),
    Event(SyncTimelineEntry),
    ActiveTurnMetadata(SyncActiveTurnMetadata),
    AcceptedCommand(SyncAcceptedCommand),
    Reset {
        history_generation: u64,
    },
    Delete,
}
