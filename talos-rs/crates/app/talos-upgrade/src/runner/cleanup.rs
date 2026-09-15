use super::{
    unix_seconds, write_state, UpgradeContext, UpgradeState, UpgradeTask, UpgradeTaskStatus,
};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct UpgradeCleanupState {
    status: CleanupStatus,
    // Legacy records inherit the task timestamp when loaded.
    pub(super) updated_at: i64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum CleanupStatus {
    #[default]
    Pending,
    Completed,
}

impl UpgradeCleanupState {
    pub(super) fn pending(updated_at: i64) -> Self {
        Self {
            status: CleanupStatus::Pending,
            updated_at,
        }
    }

    pub(super) fn completed(updated_at: i64) -> Self {
        Self {
            status: CleanupStatus::Completed,
            updated_at,
        }
    }
}

/// Cleanup never invalidates a committed migration. Publish each completion
/// through a separate candidate so failed writes cannot leak into later saves.
pub(super) async fn run(
    tasks: &[&dyn UpgradeTask],
    context: &UpgradeContext<'_>,
    state_path: &Path,
    state: &mut UpgradeState,
) {
    for task in tasks {
        let id = task.definition().id;
        let Some(task_state) = state.tasks.get(id).cloned() else {
            continue;
        };
        if task.definition().target_version > super::task_target_ceiling(&state.product_version)
            || task_state.status != UpgradeTaskStatus::Success
            || task_state.cleanup.status == CleanupStatus::Completed
        {
            continue;
        }
        let result = async {
            task.cleanup(context).await?;
            let mut completed = task_state;
            completed.cleanup = UpgradeCleanupState::completed(unix_seconds()?);
            let mut candidate = state.clone();
            candidate.tasks.insert(id.to_owned(), completed);
            write_state(state_path, &candidate)?;
            *state = candidate;
            Ok::<(), talos_error::TalosError>(())
        }
        .await;
        match result {
            Ok(()) => tracing::info!(upgrade_id = id, "upgrade task cleanup completed"),
            Err(error) => talos_logging::report_error(&error.context(format!(
                "upgrade task {id} cleanup pending; will retry on startup"
            ))),
        }
    }
}
