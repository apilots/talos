//! Durable Session facts. The transaction records references to canonical
//! events and command receipts without projecting or writing an outbox row.
use std::{collections::BTreeMap, sync::Arc};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use talos_error::{TalosError, TalosResult};
use talos_storage::{Row, StorageTransaction};

use super::StoreSyncBindings;

pub(crate) async fn retain_deleted_events(
    tx: &mut dyn StorageTransaction,
    session_ids: &[&str],
) -> TalosResult<()> {
    for id in session_ids {
        retire_history(tx, id, /*deleted*/ true).await?;
    }
    Ok(())
}

pub(crate) async fn retire_history(
    tx: &mut dyn StorageTransaction,
    session_id: &str,
    deleted: bool,
) -> TalosResult<()> {
    tx.execute(
        "INSERT INTO session_event_retention(session_id,through_generation,deleted)
         SELECT session_key,history_generation,? FROM sessions WHERE session_key=?
         ON CONFLICT(session_id) DO UPDATE SET
             through_generation=MAX(session_event_retention.through_generation,excluded.through_generation),
             deleted=MAX(session_event_retention.deleted,excluded.deleted), reclaim=1",
        &[if deleted { "1" } else { "0" }, session_id],
    ).await?;
    Ok(())
}

#[derive(Clone, Serialize, Deserialize, PartialEq)]
pub(super) struct SourceFields(BTreeMap<String, Value>);

impl From<Row> for SourceFields {
    fn from(row: Row) -> Self {
        Self(row.columns.into_iter().zip(row.values).collect())
    }
}

impl SourceFields {
    pub(super) fn set_text(&mut self, field: &str, value: String) {
        self.0.insert(field.to_owned(), Value::String(value));
    }

    pub(super) fn row(&self) -> Row {
        let (columns, values) = self
            .0
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .unzip();
        Row::new(columns, values)
    }
}

#[derive(Serialize, Deserialize, PartialEq)]
pub(super) struct ActiveState {
    pub(super) turn_id: String,
    pub(super) state: talos_agent_api::ActiveTurnState,
    pub(super) event_seq: i64,
    pub(super) updated_at: i64,
}

#[derive(Serialize, Deserialize, PartialEq)]
pub(super) struct SourceCut {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) fields: Option<SourceFields>,
    pub(super) generation: u64,
    pub(super) event_tail: i64,
    pub(super) command_tail: i64,
    pub(super) active: Option<ActiveState>,
}

#[derive(Serialize, Deserialize)]
pub(super) struct SessionRevision {
    pub(super) before: Option<SourceCut>,
    pub(super) after: Option<SourceCut>,
}

async fn cut(tx: &mut dyn StorageTransaction, session_id: &str) -> TalosResult<Option<SourceCut>> {
    let Some(row) = tx
        .query(
            "SELECT * FROM sessions WHERE session_key = ?",
            &[session_id],
        )
        .await?
        .into_iter()
        .next()
    else {
        return Ok(None);
    };
    Ok(Some(cut_from_row(tx, session_id, row).await?))
}

pub(super) async fn cut_from_row(
    tx: &mut dyn StorageTransaction,
    session_id: &str,
    row: Row,
) -> TalosResult<SourceCut> {
    let generation = super::metadata::number(&row, "history_generation")? as u64;
    let event_tail = super::timeline::tail(tx, session_id, generation).await?;
    let command_tail = super::timeline::command_tail(tx, session_id, generation).await?;
    let active = tx
        .query(
            "SELECT * FROM agent_active_turns WHERE session_id = ?",
            &[session_id],
        )
        .await?
        .first()
        .map(crate::session::agent_execution::record::parse_active_turn)
        .transpose()?
        .map(|record| ActiveState {
            turn_id: record.identity.turn_id,
            state: record.state,
            event_seq: record.last_event_seq,
            updated_at: record.updated_at,
        });
    Ok(SourceCut {
        fields: Some(row.into()),
        generation,
        event_tail,
        command_tail,
        active,
    })
}

struct SessionTransaction {
    tx: Box<dyn StorageTransaction>,
    bindings: Arc<StoreSyncBindings>,
    cuts: Vec<(String, Option<SourceCut>)>,
}

pub(crate) async fn transaction(
    mut tx: Box<dyn StorageTransaction>,
    bindings: Arc<StoreSyncBindings>,
    session_ids: Vec<String>,
) -> TalosResult<Box<dyn StorageTransaction>> {
    if bindings.is_empty() {
        return Ok(tx);
    }
    let mut cuts = Vec::with_capacity(session_ids.len());
    for id in session_ids {
        let before = cut(&mut *tx, &id).await?;
        cuts.push((id, before));
    }
    Ok(Box::new(SessionTransaction { tx, bindings, cuts }))
}

#[async_trait]
impl StorageTransaction for SessionTransaction {
    async fn query(&mut self, sql: &str, params: &[&str]) -> TalosResult<Vec<Row>> {
        self.tx.query(sql, params).await
    }

    async fn execute(&mut self, sql: &str, params: &[&str]) -> TalosResult<u64> {
        self.tx.execute(sql, params).await
    }

    async fn rollback(self: Box<Self>) -> TalosResult<()> {
        self.tx.rollback().await
    }

    async fn commit(mut self: Box<Self>) -> TalosResult<()> {
        let bindings = self.bindings.all();
        let mut recorded = false;
        for (id, mut before) in std::mem::take(&mut self.cuts) {
            let mut after = cut(&mut *self.tx, &id).await?;
            let source = match (&before, &after) {
                (None, None) => continue,
                (Some(before), Some(after)) if before == after => continue,
                (before, after) => after.as_ref().or(before.as_ref()).expect("one cut exists"),
            };
            let row = source.fields.as_ref().expect("captured Session row").row();
            let user = super::metadata::text(&row, "user_id")?;
            if !bindings.iter().any(|adapter| {
                user == adapter.binding.source_user_id || user == adapter.binding.scope.account_id
            }) {
                continue;
            }
            if let (Some(before), Some(after)) = (&mut before, &mut after) {
                // Keep a pre-mutation baseline for the first revision. Later
                // event-only updates need only cursors and active state.
                if before.fields == after.fields
                    && !self.tx.query(
                        "SELECT 1 FROM resource_revisions WHERE owner_user_id=? AND kind='session' AND resource_id=? LIMIT 1",
                        &[&user, &id],
                    ).await?.is_empty()
                {
                    before.fields = None;
                    after.fields = None;
                }
            }
            let raw =
                serde_json::to_string(&SessionRevision { before, after }).map_err(|error| {
                    TalosError::internal_with_source("Encode Session revision.", error)
                })?;
            self.tx.execute(
                "INSERT INTO resource_revisions(kind, resource_id, owner_user_id, execution_node_id, changes_json) VALUES ('session', ?, ?, ?, ?)",
                &[&id, &user, &super::metadata::text(&row, "execution_node_id")?, &raw],
            ).await?;
            recorded = true;
        }
        self.tx.commit().await?;
        if recorded {
            self.bindings.wake_capture();
        }
        Ok(())
    }
}
