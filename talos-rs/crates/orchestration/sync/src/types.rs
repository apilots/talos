use serde::{Deserialize, Serialize};
use std::time::Duration;
use talos_agent_api::{ActiveTurnState, TimelineEventPayload};
use talos_types::{
    AgentTurnRef, FileInfo, IdempotentCommand, NodeInfo, ProjectInfo, SessionConf, SessionInfo,
    SessionTreeIdentity, TalosSessionEnvironment, TurnCommandStatus,
};

pub use talos_sync_types::{
    ComputerAlias, SessionExecutionBinding, SyncBinding, SyncResourceKey, SyncScope, SyncVersion,
};

/// Local configuration. Account credentials remain in the existing auth owner.
#[derive(Debug, Clone)]
pub struct SyncOptions {
    pub hub_endpoint: String,
}

/// Already-authorized source association supplied by the management host.
#[derive(Debug, Clone)]
pub struct SyncBindParams {
    pub scope: SyncScope,
    pub source_user_id: String,
    pub computers: Vec<ComputerAlias>,
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

/// Facts appended inside the caller's existing business transaction.
pub struct SyncCaptureParams {
    pub binding: SyncBinding,
    pub resource: SyncResourceKey,
    pub changes: Vec<SyncChange>,
    pub created_at: i64,
}

/// Durable upload candidate; network delivery alone never removes this batch.
pub struct SyncExportBatch {
    pub scope: SyncScope,
    pub resource: SyncResourceKey,
    pub through_sequence: i64,
    pub update: Vec<u8>,
    pub version: SyncVersion,
}

/// A local cursor mapping owned by the same transaction as imported facts.
pub struct SyncEventMapping {
    pub scope: SyncScope,
    pub session_id: String,
    pub generation: u64,
    pub key: SyncEventKey,
    pub local_event_id: String,
    pub local_event_seq: i64,
}

/// One authenticated account connection. Credentials remain owned by original Auth.
pub struct SyncStartParams {
    pub binding: SyncBinding,
    pub credentials: talos_sync_sdk::CredentialCallback,
    pub trusted_ca_pem: Option<Vec<u8>>,
    pub reconnect_delay: Duration,
}

pub struct SyncScanParams {
    pub binding: SyncBinding,
    pub cursor: Option<String>,
    pub limit: usize,
}

pub struct SyncScanPage {
    pub resources: Vec<SyncResourceKey>,
    pub next_cursor: Option<String>,
}

pub struct SyncExportParams {
    pub binding: SyncBinding,
    pub resource: SyncResourceKey,
}

/// Already validated shared facts, materialized by existing Store owners.
pub struct SyncImportBatch {
    pub scope: SyncScope,
    pub resource: SyncResourceKey,
    pub version: SyncVersion,
    pub changes: Vec<talos_sync_types::SyncChange>,
}

pub use talos_types::SyncConnectionState;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncStatus {
    pub connection: SyncConnectionState,
    pub pending_changes: u64,
    pub last_acknowledged_at: Option<i64>,
    pub last_error: Option<talos_error::ApiError>,
}
