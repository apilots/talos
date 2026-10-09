use async_trait::async_trait;
use sqlx::{sqlite::SqliteRow, Row, SqlitePool};
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_storage::database_error;

use agent_protocol::{NodeHealth, NodeMode, NodeStatus};

use super::{
    CreateNodeParams, HeartbeatNodeParams, LeaseCheck, ListNodesParams, NodePageRecord, NodeRecord,
    RegisterNodeParams, RenewNodeParams, RotateCredentialParams, UpdateNodeParams,
};

const NODE_COLUMNS: &str = "node_id, owner_user_id, display_name, mode, endpoint, status, credential_hash, credential_expires_at, lease_token_hash, lease_expires_at, last_heartbeat_at, health, created_at, updated_at";

#[async_trait]
pub trait AgentNodeStore: Send + Sync {
    async fn upsert_embedded(&self, status: NodeStatus, now_at: i64) -> TalosResult<NodeRecord>;
    async fn create(&self, params: CreateNodeParams) -> TalosResult<NodeRecord>;
    async fn get(&self, node_id: &str) -> TalosResult<Option<NodeRecord>>;
    async fn list(&self, params: ListNodesParams) -> TalosResult<NodePageRecord>;
    async fn register(&self, params: RegisterNodeParams) -> TalosResult<NodeRecord>;
    async fn renew(&self, params: RenewNodeParams) -> TalosResult<NodeRecord>;
    async fn heartbeat(&self, params: HeartbeatNodeParams) -> TalosResult<NodeRecord>;
    async fn unregister(&self, lease: LeaseCheck) -> TalosResult<()>;
    async fn update(&self, params: UpdateNodeParams) -> TalosResult<NodeRecord>;
    async fn rotate_credential(&self, params: RotateCredentialParams) -> TalosResult<NodeRecord>;
}

pub struct DatabaseAgentNodeStore {
    pool: SqlitePool,
}

impl DatabaseAgentNodeStore {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl AgentNodeStore for DatabaseAgentNodeStore {
    async fn upsert_embedded(&self, status: NodeStatus, now_at: i64) -> TalosResult<NodeRecord> {
        let row = sqlx::query(&format!(
            "INSERT INTO agent_nodes (node_id, owner_user_id, display_name, mode, endpoint, status, created_at, updated_at) VALUES ('embedded', NULL, 'Embedded Agent', 'embedded', NULL, ?, ?, ?) ON CONFLICT(node_id) DO UPDATE SET status = excluded.status, updated_at = excluded.updated_at WHERE agent_nodes.mode = 'embedded' RETURNING {NODE_COLUMNS}"
        ))
        .bind(status_text(status))
        .bind(now_at)
        .bind(now_at)
        .fetch_all(&self.pool)
        .await
        .map_err(|error| database_error("register embedded Agent node", error))?;
        row.first()
            .ok_or_else(|| {
                TalosError::new(
                    ErrorCode::Conflict,
                    "Embedded Agent node identity is occupied",
                )
            })
            .and_then(record_from_row)
    }

    async fn create(&self, params: CreateNodeParams) -> TalosResult<NodeRecord> {
        let result = sqlx::query(&format!(
            "INSERT INTO agent_nodes (node_id, owner_user_id, display_name, mode, endpoint, credential_hash, credential_expires_at, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?) RETURNING {NODE_COLUMNS}"
        ))
        .bind(&params.node_id)
        .bind(&params.owner_user_id)
        .bind(&params.display_name)
        .bind(mode_text(params.mode))
        .bind(&params.endpoint)
        .bind(params.credential_hash.as_ref().map(|hash| hash.as_slice()))
        .bind(params.credential_expires_at)
        .bind(params.now_at)
        .bind(params.now_at)
        .fetch_all(&self.pool)
        .await;
        let rows = result.map_err(|error| {
            if is_unique_violation(&error) {
                TalosError::new(ErrorCode::Conflict, "Agent node already exists").with_source(error)
            } else {
                database_error("create Agent node", error)
            }
        })?;
        rows.first()
            .ok_or_else(|| TalosError::internal("Agent node insert returned no row"))
            .and_then(record_from_row)
    }

    async fn get(&self, node_id: &str) -> TalosResult<Option<NodeRecord>> {
        let row = sqlx::query(&format!(
            "SELECT {NODE_COLUMNS} FROM agent_nodes WHERE node_id = ?"
        ))
        .bind(node_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|error| database_error("read Agent node", error))?;
        row.as_ref().map(record_from_row).transpose()
    }

    async fn list(&self, params: ListNodesParams) -> TalosResult<NodePageRecord> {
        let limit = i64::from(params.limit.clamp(1, 200)) + 1;
        let rows = sqlx::query(&format!(
            "SELECT {NODE_COLUMNS} FROM agent_nodes WHERE (? IS NULL OR owner_user_id = ?) AND (? IS NULL OR node_id > ?) ORDER BY node_id LIMIT ?"
        ))
        .bind(params.owner_user_id.as_deref())
        .bind(params.owner_user_id.as_deref())
        .bind(params.cursor.as_deref())
        .bind(params.cursor.as_deref())
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(|error| database_error("list Agent nodes", error))?;
        let mut data = rows
            .iter()
            .map(record_from_row)
            .collect::<TalosResult<Vec<_>>>()?;
        let has_more = data.len() as i64 == limit;
        if has_more {
            data.pop();
        }
        let next_cursor = if has_more {
            data.last().map(|node| node.node_id.clone())
        } else {
            None
        };
        Ok(NodePageRecord { data, next_cursor })
    }

    async fn register(&self, params: RegisterNodeParams) -> TalosResult<NodeRecord> {
        let row = sqlx::query(&format!(
            "UPDATE agent_nodes SET lease_token_hash = ?, lease_expires_at = ?, last_heartbeat_at = NULL, health = 'unknown', updated_at = ? WHERE node_id = ? AND mode = 'remote' AND status = 'enabled' AND credential_hash = ? AND credential_expires_at > ? RETURNING {NODE_COLUMNS}"
        ))
        .bind(params.lease_token_hash.as_slice())
        .bind(params.lease_expires_at)
        .bind(params.now_at)
        .bind(&params.node_id)
        .bind(params.expected_credential_hash.as_slice())
        .bind(params.now_at)
        .fetch_all(&self.pool)
        .await
        .map_err(|error| database_error("register Agent node", error))?;
        record_or_expired(row.into_iter().next())
    }

    async fn renew(&self, params: RenewNodeParams) -> TalosResult<NodeRecord> {
        let row = sqlx::query(&format!(
            "UPDATE agent_nodes SET lease_expires_at = ?, updated_at = ? WHERE node_id = ? AND status = 'enabled' AND lease_token_hash = ? AND lease_expires_at > ? AND lease_expires_at <= ? AND credential_expires_at >= ? RETURNING {NODE_COLUMNS}"
        ))
        .bind(params.expires_at)
        .bind(params.lease.now_at)
        .bind(&params.lease.node_id)
        .bind(params.lease.token_hash.as_slice())
        .bind(params.lease.now_at)
        .bind(params.expires_at)
        .bind(params.expires_at)
        .fetch_all(&self.pool)
        .await
        .map_err(|error| database_error("renew Agent node lease", error))?;
        record_or_expired(row.into_iter().next())
    }

    async fn heartbeat(&self, params: HeartbeatNodeParams) -> TalosResult<NodeRecord> {
        let row = sqlx::query(&format!(
            "UPDATE agent_nodes SET last_heartbeat_at = ?, health = ?, updated_at = ? WHERE node_id = ? AND status = 'enabled' AND lease_token_hash = ? AND lease_expires_at > ? RETURNING {NODE_COLUMNS}"
        ))
        .bind(params.lease.now_at)
        .bind(health_text(params.health))
        .bind(params.lease.now_at)
        .bind(&params.lease.node_id)
        .bind(params.lease.token_hash.as_slice())
        .bind(params.lease.now_at)
        .fetch_all(&self.pool)
        .await
        .map_err(|error| database_error("record Agent node heartbeat", error))?;
        record_or_expired(row.into_iter().next())
    }

    async fn unregister(&self, lease: LeaseCheck) -> TalosResult<()> {
        let changed = sqlx::query(
            "UPDATE agent_nodes SET lease_token_hash = NULL, lease_expires_at = NULL, last_heartbeat_at = NULL, health = 'unknown', updated_at = ? WHERE node_id = ? AND lease_token_hash = ? AND lease_expires_at > ?",
        )
        .bind(lease.now_at)
        .bind(&lease.node_id)
        .bind(lease.token_hash.as_slice())
        .bind(lease.now_at)
        .execute(&self.pool)
        .await
        .map_err(|error| database_error("unregister Agent node", error))?;
        if changed.rows_affected() == 0 {
            return Err(expired());
        }
        Ok(())
    }

    async fn update(&self, params: UpdateNodeParams) -> TalosResult<NodeRecord> {
        let status = status_text(params.status);
        let disabled = i64::from(params.status == NodeStatus::Disabled);
        let row = sqlx::query(&format!(
            "UPDATE agent_nodes SET display_name = ?, endpoint = ?, status = ?, lease_token_hash = CASE WHEN endpoint IS NOT ? OR ? = 1 THEN NULL ELSE lease_token_hash END, lease_expires_at = CASE WHEN endpoint IS NOT ? OR ? = 1 THEN NULL ELSE lease_expires_at END, last_heartbeat_at = CASE WHEN endpoint IS NOT ? OR ? = 1 THEN NULL ELSE last_heartbeat_at END, health = CASE WHEN endpoint IS NOT ? OR ? = 1 THEN 'unknown' ELSE health END, updated_at = ? WHERE node_id = ? AND mode = 'remote' RETURNING {NODE_COLUMNS}"
        ))
        .bind(&params.display_name)
        .bind(&params.endpoint)
        .bind(status)
        .bind(&params.endpoint)
        .bind(disabled)
        .bind(&params.endpoint)
        .bind(disabled)
        .bind(&params.endpoint)
        .bind(disabled)
        .bind(&params.endpoint)
        .bind(disabled)
        .bind(params.now_at)
        .bind(&params.node_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|error| database_error("update Agent node", error))?;
        record_or_missing(row.into_iter().next())
    }

    async fn rotate_credential(&self, params: RotateCredentialParams) -> TalosResult<NodeRecord> {
        let row = sqlx::query(&format!(
            "UPDATE agent_nodes SET credential_hash = ?, credential_expires_at = ?, lease_token_hash = NULL, lease_expires_at = NULL, last_heartbeat_at = NULL, health = 'unknown', updated_at = ? WHERE node_id = ? AND mode = 'remote' RETURNING {NODE_COLUMNS}"
        ))
        .bind(params.hash.as_slice())
        .bind(params.expires_at)
        .bind(params.now_at)
        .bind(&params.node_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|error| database_error("rotate Agent node credential", error))?;
        record_or_missing(row.into_iter().next())
    }
}

fn record_from_row(row: &SqliteRow) -> TalosResult<NodeRecord> {
    let text = |column| {
        row.try_get::<String, _>(column)
            .map_err(|error| database_error("decode Agent node", error))
    };
    let optional_hash = |column| -> TalosResult<Option<[u8; 32]>> {
        let raw: Option<Vec<u8>> = row
            .try_get(column)
            .map_err(|error| database_error("decode Agent node hash", error))?;
        raw.map(|bytes| {
            bytes
                .try_into()
                .map_err(|_| TalosError::internal("Agent node hash has invalid length"))
        })
        .transpose()
    };
    Ok(NodeRecord {
        node_id: text("node_id")?,
        owner_user_id: row
            .try_get("owner_user_id")
            .map_err(|error| database_error("decode Agent node owner", error))?,
        display_name: text("display_name")?,
        mode: parse_mode(&text("mode")?)?,
        endpoint: row
            .try_get("endpoint")
            .map_err(|error| database_error("decode Agent node endpoint", error))?,
        status: parse_status(&text("status")?)?,
        credential_hash: optional_hash("credential_hash")?,
        credential_expires_at: row
            .try_get("credential_expires_at")
            .map_err(|error| database_error("decode Agent node credential expiry", error))?,
        lease_token_hash: optional_hash("lease_token_hash")?,
        lease_expires_at: row
            .try_get("lease_expires_at")
            .map_err(|error| database_error("decode Agent node lease expiry", error))?,
        last_heartbeat_at: row
            .try_get("last_heartbeat_at")
            .map_err(|error| database_error("decode Agent node heartbeat", error))?,
        health: parse_health(&text("health")?)?,
        created_at: row
            .try_get("created_at")
            .map_err(|error| database_error("decode Agent node creation time", error))?,
        updated_at: row
            .try_get("updated_at")
            .map_err(|error| database_error("decode Agent node update time", error))?,
    })
}

fn mode_text(mode: NodeMode) -> &'static str {
    match mode {
        NodeMode::Embedded => "embedded",
        NodeMode::Remote => "remote",
    }
}

fn status_text(status: NodeStatus) -> &'static str {
    match status {
        NodeStatus::Enabled => "enabled",
        NodeStatus::Disabled => "disabled",
    }
}

fn health_text(health: NodeHealth) -> &'static str {
    match health {
        NodeHealth::Unknown => "unknown",
        NodeHealth::Healthy => "healthy",
        NodeHealth::Degraded => "degraded",
    }
}

fn parse_mode(value: &str) -> TalosResult<NodeMode> {
    match value {
        "embedded" => Ok(NodeMode::Embedded),
        "remote" => Ok(NodeMode::Remote),
        _ => Err(TalosError::internal("Agent node mode is invalid")),
    }
}

fn parse_status(value: &str) -> TalosResult<NodeStatus> {
    match value {
        "enabled" => Ok(NodeStatus::Enabled),
        "disabled" => Ok(NodeStatus::Disabled),
        _ => Err(TalosError::internal("Agent node status is invalid")),
    }
}

fn parse_health(value: &str) -> TalosResult<NodeHealth> {
    match value {
        "unknown" => Ok(NodeHealth::Unknown),
        "healthy" => Ok(NodeHealth::Healthy),
        "degraded" => Ok(NodeHealth::Degraded),
        _ => Err(TalosError::internal("Agent node health is invalid")),
    }
}

fn record_or_expired(row: Option<SqliteRow>) -> TalosResult<NodeRecord> {
    row.as_ref().ok_or_else(expired).and_then(record_from_row)
}

fn record_or_missing(row: Option<SqliteRow>) -> TalosResult<NodeRecord> {
    row.as_ref()
        .ok_or_else(|| TalosError::new(ErrorCode::ResourceNotFound, "Agent node not found"))
        .and_then(record_from_row)
}

fn expired() -> TalosError {
    TalosError::new(
        ErrorCode::ResourceExpired,
        "Agent node registration is not current",
    )
}

fn is_unique_violation(error: &sqlx::Error) -> bool {
    matches!(error, sqlx::Error::Database(database) if database.is_unique_violation())
}
