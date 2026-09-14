//! SQLite repair of pre-0.1.8 execution history and Provider workspace files.
//! Runtime readers remain canonical-only. SQL is committed after durable files;
//! immutable file backups make an interrupted pre-commit attempt repeatable.
use semver::Version;
use sqlx::Executor;
use talos_error::{TalosError, TalosResult};

use crate::runner::{
    UpgradeContext, UpgradeFuture, UpgradeTask, UpgradeTaskDefinition, UpgradeTaskRegistration,
};

#[path = "history_repair/artifacts_0_1_8.rs"]
mod artifacts;
#[path = "history_repair/database_0_1_8.rs"]
mod database;
#[path = "history_repair/events_0_1_8.rs"]
mod events;
#[path = "history_repair/files_0_1_8.rs"]
mod files;

const UPGRADE_ID: &str = "202609140001_history_repair_0_1_8";

pub(crate) fn upgrade_task() -> Box<dyn UpgradeTask> {
    Box::new(UpgradeTaskRegistration::new(
        UpgradeTaskDefinition {
            id: UPGRADE_ID,
            revision: 1,
            target_version: Version::new(0, 1, 8),
            priority: 200,
        },
        run,
    ))
}

pub(crate) async fn backup_database(
    config: &talos_config::Config,
    pool: &sqlx::SqlitePool,
) -> TalosResult<()> {
    database::backup_legacy_database(
        pool,
        &config
            .talos_paths()
            .talos_home
            .join("upgrades")
            .join(UPGRADE_ID),
    )
    .await
}

fn run<'a>(context: &'a UpgradeContext<'_>) -> UpgradeFuture<'a> {
    Box::pin(async move {
        let mut transaction = context.database_pool.begin().await.map_err(source_error)?;
        let mut repair = database::prepare(&mut transaction).await?;
        let file_repairs = artifacts::prepare(context, &mut repair, &mut transaction).await?;
        if repair.legacy_exists {
            repair.apply(&mut transaction).await?;
        }
        for file in &file_repairs {
            file.apply()?;
        }
        if repair.legacy_exists {
            (&mut *transaction)
                .execute(include_str!("history_repair/history_repair_0_1_8.sql"))
                .await
                .map_err(source_error)?;
        }
        transaction.commit().await.map_err(source_error)?;
        tracing::info!(
            upgrade_id = UPGRADE_ID,
            files = file_repairs.len(),
            "history and workspace repair committed"
        );
        Ok(())
    })
}

fn invalid(detail: impl Into<String>) -> TalosError {
    TalosError::internal("Upgrade data validation failed; technical details were withheld.")
        .context(detail.into())
}
fn source_error(error: impl std::error::Error + Send + Sync + 'static) -> TalosError {
    TalosError::internal_with_source(
        "Upgrade data repair failed; technical details were withheld.",
        error,
    )
}
fn decode<T: serde::de::DeserializeOwned>(value: serde_json::Value) -> TalosResult<T> {
    serde_json::from_value(value).map_err(source_error)
}
