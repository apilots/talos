//! Durable pause control, scoped to the real callback execution instance.
//! Definitions and observed outcomes remain owned by the original scheduler.
use chrono::Utc;
use sqlx::{Row, SqlitePool};
use std::{future::Future, pin::Pin};
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_types::BuiltinJobId;

pub type NativeJobStoreFuture<'a, T> = Pin<Box<dyn Future<Output = TalosResult<T>> + Send + 'a>>;

/// A command parameter, not a public database record or authorization proof.
pub struct SetNativeJobPaused<'a> {
    pub execution_instance_id: &'a str,
    pub job_id: BuiltinJobId,
    pub paused: bool,
}

pub trait NativeJobControlStore: Send + Sync {
    /// Load before scheduler startup, including callbacks marked run-on-start.
    fn load_paused<'a>(
        &'a self,
        execution_instance_id: &'a str,
    ) -> NativeJobStoreFuture<'a, Vec<BuiltinJobId>>;
    fn set_paused<'a>(&'a self, update: SetNativeJobPaused<'a>) -> NativeJobStoreFuture<'a, ()>;
}

pub struct DatabaseNativeJobControlStore {
    pool: SqlitePool,
}
impl DatabaseNativeJobControlStore {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

impl NativeJobControlStore for DatabaseNativeJobControlStore {
    fn load_paused<'a>(
        &'a self,
        execution_instance_id: &'a str,
    ) -> NativeJobStoreFuture<'a, Vec<BuiltinJobId>> {
        Box::pin(async move {
            validate_instance(execution_instance_id)?;
            let rows = sqlx::query("SELECT job_id FROM automation_native_job_controls WHERE execution_instance_id = ? AND paused = 1 ORDER BY job_id")
                .bind(execution_instance_id).fetch_all(&self.pool).await.map_err(storage_error)?;
            rows.iter()
                .map(|row| {
                    match row
                        .try_get::<String, _>("job_id")
                        .map_err(storage_error)?
                        .as_str()
                    {
                        "profile-usage-retention" => Ok(BuiltinJobId::ProfileUsageRetention),
                        "dream-memory-maintenance" => Ok(BuiltinJobId::DreamMemoryMaintenance),
                        _ => Err(TalosError::internal(
                            "Stored native job identity is invalid.",
                        )),
                    }
                })
                .collect()
        })
    }
    fn set_paused<'a>(&'a self, update: SetNativeJobPaused<'a>) -> NativeJobStoreFuture<'a, ()> {
        Box::pin(async move {
            validate_instance(update.execution_instance_id)?;
            sqlx::query("INSERT INTO automation_native_job_controls(execution_instance_id, job_id, paused, updated_at) VALUES (?, ?, ?, ?) ON CONFLICT(execution_instance_id, job_id) DO UPDATE SET paused = excluded.paused, updated_at = excluded.updated_at")
                .bind(update.execution_instance_id).bind(update.job_id.as_str()).bind(update.paused).bind(Utc::now().timestamp_millis())
                .execute(&self.pool).await.map_err(storage_error)?;
            Ok(())
        })
    }
}
fn validate_instance(instance: &str) -> TalosResult<()> {
    if instance.trim().is_empty() {
        Err(TalosError::new(
            ErrorCode::InvalidRequest,
            "An execution instance is required.",
        ))
    } else {
        Ok(())
    }
}
fn storage_error(error: sqlx::Error) -> TalosError {
    TalosError::internal_with_source(
        "Native job control storage failed; technical details were withheld.",
        error,
    )
}
#[cfg(test)]
#[path = "native_jobs_tests.rs"]
mod tests;
