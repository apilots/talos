//! Patch persisted Session records atomically in SQLite.

use talos_error::{TalosError, TalosResult};
use talos_types::{SessionUsage, DEFAULT_SESSION_TITLE};

use super::DatabaseSessionStore;
use crate::session::{PatchSessionParams, SessionRecord, SessionRecordPatch};

impl DatabaseSessionStore {
    pub(super) async fn clear_session_history_record(
        &self,
        session_id: &str,
        cleared_at: u64,
    ) -> TalosResult<bool> {
        let mut transaction = self.begin_session_transaction(session_id).await?;
        let active = transaction
            .query(
                "SELECT 1 AS active FROM agent_active_turns WHERE session_id = ? LIMIT 1",
                &[session_id],
            )
            .await?;
        if !active.is_empty() {
            transaction.rollback().await?;
            return Err(TalosError::new(
                talos_error::ErrorCode::ActiveTurnRunning,
                "cannot clear a Session with an active Agent Turn",
            ));
        }

        super::super::worker_projection::ensure_settled(&mut *transaction, &[session_id]).await?;

        transaction
            .execute(
                "DELETE FROM agent_command_idempotency WHERE session_id = ?",
                &[session_id],
            )
            .await?;
        transaction
            .execute(
                "DELETE FROM worker_projection_cursors WHERE session_id = ?",
                &[session_id],
            )
            .await?;
        transaction
            .execute(
                "DELETE FROM session_dream_cursors WHERE session_id=?",
                &[session_id],
            )
            .await?;
        crate::capture::source::retire_history(
            &mut *transaction,
            session_id,
            /*deleted*/ false,
        )
        .await?;
        let usage_json = Self::build_usage_json(&SessionUsage::default())?;
        let cleared_at = cleared_at.to_string();
        let affected = transaction
            .execute(
                r#"
                UPDATE sessions
                SET title = ?, last_activity = ?, message_count = 0,
                    latest_turn_seq = 0, memory_processed_until_event_seq = 0,
                    history_generation = history_generation + 1,
                    usage_json = ?
                WHERE session_key = ?
                "#,
                &[
                    DEFAULT_SESSION_TITLE,
                    cleared_at.as_str(),
                    usage_json.as_str(),
                    session_id,
                ],
            )
            .await?;
        if affected == 0 {
            transaction.rollback().await?;
            return Ok(false);
        }
        transaction.commit().await?;
        Ok(true)
    }

    pub(super) async fn patch_session_record(
        &self,
        params: PatchSessionParams,
    ) -> TalosResult<Option<SessionRecord>> {
        let mut transaction = self
            .begin_session_transaction(params.session_id.as_str())
            .await?;
        if !matches!(params.change, SessionRecordPatch::Pinned(_)) {
            let active = transaction
                .query(
                    "SELECT 1 AS active FROM agent_active_turns WHERE session_id = ? LIMIT 1",
                    &[params.session_id.as_str()],
                )
                .await?;
            if !active.is_empty() {
                transaction.rollback().await?;
                return Err(TalosError::new(
                    talos_error::ErrorCode::ActiveTurnRunning,
                    format!("session `{}` already has an active turn", params.session_id),
                ));
            }
        }

        let last_activity = params.last_activity.to_string();
        let affected = match params.change {
            SessionRecordPatch::Settings {
                conf,
                current_directory,
                pinned,
            } => {
                let conf_json = Self::build_conf_json(&conf)?;
                let directory = current_directory.map(|path| path.display().to_string());
                if let Some(pinned) = pinned {
                    let pinned_flag = if pinned { "1" } else { "0" };
                    transaction
                        .execute(
                            "UPDATE sessions SET conf_json = ?, current_directory = NULLIF(?, ''), pinned = ?, last_activity = ? WHERE session_key = ?",
                            &[
                                &conf_json,
                                directory.as_deref().unwrap_or_default(),
                                pinned_flag,
                                &last_activity,
                                &params.session_id,
                            ],
                        )
                        .await?
                } else {
                    transaction
                        .execute(
                            "UPDATE sessions SET conf_json = ?, current_directory = NULLIF(?, ''), last_activity = ? WHERE session_key = ?",
                            &[
                                &conf_json,
                                directory.as_deref().unwrap_or_default(),
                                &last_activity,
                                &params.session_id,
                            ],
                        )
                        .await?
                }
            }
            SessionRecordPatch::Pinned(pinned) => {
                let pinned_flag = if pinned { "1" } else { "0" };
                transaction
                    .execute(
                        "UPDATE sessions SET pinned = ? WHERE session_key = ?",
                        &[pinned_flag, &params.session_id],
                    )
                    .await?
            }
            SessionRecordPatch::Worktree(binding) => {
                let binding = Self::serialize_worktree_binding(&binding)?;
                transaction
                    .execute(
                        "UPDATE sessions SET worktree_binding_json = ?, last_activity = ? WHERE session_key = ?",
                        &[&binding, &last_activity, &params.session_id],
                    )
                    .await?
            }
        };
        if affected == 0 {
            transaction.rollback().await?;
            return Ok(None);
        }
        let record =
            Self::fetch_session_in_transaction(transaction.as_mut(), params.session_id.as_str())
                .await?;
        transaction.commit().await?;
        Ok(record)
    }
}
