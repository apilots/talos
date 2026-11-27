//! Patch persisted Session records atomically in SQLite.

use talos_error::{TalosError, TalosResult};

use super::DatabaseSessionStore;
use crate::session::{PatchSessionParams, SessionRecord, SessionRecordPatch};

impl DatabaseSessionStore {
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
