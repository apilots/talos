//! Durable Session facts. The transaction records references to canonical
//! events and command receipts without projecting or writing an outbox row.
use std::{collections::BTreeMap, sync::Arc};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use talos_error::{TalosError, TalosResult};
use talos_storage::{Row, StorageTransaction};

use super::StoreCaptureBindings;

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
pub(crate) struct SourceFields(BTreeMap<String, Value>);

impl From<Row> for SourceFields {
    fn from(row: Row) -> Self {
        Self(row.columns.into_iter().zip(row.values).collect())
    }
}

impl SourceFields {
    #[cfg(feature = "sync")]
    pub(crate) fn set_text(&mut self, field: &str, value: String) {
        self.0.insert(field.to_owned(), Value::String(value));
    }

    pub(crate) fn row(&self) -> Row {
        let (columns, values) = self
            .0
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .unzip();
        Row::new(columns, values)
    }
}

#[derive(Serialize, Deserialize, PartialEq)]
pub(crate) struct ActiveState {
    pub(crate) turn_id: String,
    pub(crate) state: talos_agent_api::ActiveTurnState,
    pub(crate) event_seq: i64,
    pub(crate) updated_at: i64,
}

#[derive(Serialize, Deserialize, PartialEq)]
pub(crate) struct SourceCut {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) fields: Option<SourceFields>,
    pub(crate) generation: u64,
    pub(crate) event_tail: i64,
    pub(crate) command_tail: i64,
    pub(crate) active: Option<ActiveState>,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct SessionRevision {
    pub(crate) before: Option<SourceCut>,
    pub(crate) after: Option<SourceCut>,
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

pub(crate) async fn cut_from_row(
    tx: &mut dyn StorageTransaction,
    session_id: &str,
    row: Row,
) -> TalosResult<SourceCut> {
    let generation = super::number(&row, "history_generation")? as u64;
    let event_tail = super::event_tail(tx, session_id, generation).await?;
    let command_tail = super::command_tail(tx, session_id, generation).await?;
    let active = tx
        .query(
            "SELECT turn_id,state,last_event_seq,updated_at FROM agent_active_turns WHERE session_id = ?",
            &[session_id],
        )
        .await?
        .first()
        .map(|row| -> TalosResult<ActiveState> {
            Ok(ActiveState {
                turn_id: super::text(row, "turn_id")?,
                state: crate::session::agent_execution::record::parse_active_turn_state(row)?,
                event_seq: super::number(row, "last_event_seq")?,
                updated_at: super::number(row, "updated_at")?,
            })
        })
        .transpose()?;
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
    bindings: Arc<StoreCaptureBindings>,
    cuts: Vec<(String, Option<SourceCut>)>,
}

pub(crate) async fn transaction(
    mut tx: Box<dyn StorageTransaction>,
    bindings: Arc<StoreCaptureBindings>,
    session_ids: Vec<String>,
) -> TalosResult<Box<dyn StorageTransaction>> {
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
        let recorded = match self.record_changes().await {
            Ok(recorded) => recorded,
            Err(error) => {
                self.tx.rollback().await?;
                return Err(error);
            }
        };
        self.tx.commit().await?;
        if recorded {
            self.bindings.wake_capture();
        }
        Ok(())
    }
}

impl SessionTransaction {
    async fn record_changes(&mut self) -> TalosResult<bool> {
        let mut recorded = false;
        for (id, mut before) in std::mem::take(&mut self.cuts) {
            let mut after = cut(&mut *self.tx, &id).await?;
            let source = match (&before, &after) {
                (None, None) => continue,
                (Some(before), Some(after)) if before == after => continue,
                (before, after) => after.as_ref().or(before.as_ref()).expect("one cut exists"),
            };
            let row = source.fields.as_ref().expect("captured Session row").row();
            let user = super::text(&row, "user_id")?;
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
                &[&id, &user, &super::text(&row, "execution_node_id")?, &raw],
            ).await?;
            recorded = true;
        }
        Ok(recorded)
    }
}
