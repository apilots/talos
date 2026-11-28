use std::sync::Arc;

use talos_config::DreamMemoryMaintenanceConfig;
use talos_error::TalosResult;
use talos_service::TalosService;
use talos_store::profile::{ModelCallUsageUpdate, ProfileStore, ProfileUsageUpdate};
use talos_store::session::{SessionPageCursor, SessionQuery, SessionStore};
use talos_types::ModelUsageKind;

/// Result of one bounded global Dream Memory maintenance run.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct DreamMemoryMaintenanceReport {
    pub processed_sessions: u64,
    pub failed_sessions: u64,
}

/// Selects bounded Sessions and delegates formation to their concrete Provider.
#[derive(Clone)]
pub(super) struct DreamMemoryMaintenance {
    session_store: Arc<dyn SessionStore>,
    service: Arc<TalosService>,
    coordinator: Arc<dyn talos_coordinator::AgentCoordinator>,
    profiles: Arc<dyn ProfileStore>,
    config: DreamMemoryMaintenanceConfig,
}

impl DreamMemoryMaintenance {
    pub(super) fn new(
        session_store: Arc<dyn SessionStore>,
        service: Arc<TalosService>,
        coordinator: Arc<dyn talos_coordinator::AgentCoordinator>,
        profiles: Arc<dyn ProfileStore>,
        config: DreamMemoryMaintenanceConfig,
    ) -> Self {
        Self {
            session_store,
            service,
            coordinator,
            profiles,
            config,
        }
    }

    pub(super) async fn run(&self) -> TalosResult<DreamMemoryMaintenanceReport> {
        let mut report = DreamMemoryMaintenanceReport::default();
        let mut after = None;
        for _ in 0..self.config.max_batches {
            let records = self
                .session_store
                .list_session_records(&SessionQuery {
                    limit: Some(self.config.batch_size),
                    after: after.clone(),
                    ..SessionQuery::default()
                })
                .await?;
            let record_count = records.len();
            after = records.last().map(|record| SessionPageCursor {
                last_activity: record.last_activity,
                session_id: record.id.clone(),
            });
            for record in records {
                let session_id = record.id.clone();
                match self
                    .session_store
                    .ensure_worker_execution_session(&session_id)
                    .await
                {
                    Err(error) if error.is_code(talos_error::ErrorCode::InvalidRequest) => continue,
                    result => result?,
                }
                match self.process_session(record).await {
                    Ok(true) => report.processed_sessions += 1,
                    Ok(false) => {}
                    Err(error) => {
                        report.failed_sessions += 1;
                        tracing::warn!(session_id, error = %error, "Dream Memory session processing failed");
                    }
                }
            }
            if record_count < self.config.batch_size {
                break;
            }
        }
        Ok(report)
    }

    async fn process_session(
        &self,
        record: talos_store::session::SessionRecord,
    ) -> TalosResult<bool> {
        self.session_store
            .ensure_worker_execution_session(&record.id)
            .await?;
        if !self
            .session_store
            .has_pending_dream_memory(&record.id)
            .await?
        {
            return Ok(false);
        }
        let session_id = record.id.clone();
        let result = self
            .service
            .run_idle_worker_maintenance(&session_id, |context| async move {
                let record = self
                    .session_store
                    .get_session_record(&record.id)
                    .await?
                    .ok_or_else(|| {
                        talos_error::TalosError::new(
                            talos_error::ErrorCode::SessionNotFound,
                            "Dream Memory Session no longer exists.",
                        )
                    })?;
                self.session_store
                    .ensure_worker_execution_session(&record.id)
                    .await?;
                if !self
                    .session_store
                    .has_pending_dream_memory(&record.id)
                    .await?
                {
                    return Ok(false);
                }
                let harness = self.coordinator.session_harness(&context).await?;
                let after = self.session_store.dream_memory_cursor(&record.id).await?;
                let result = harness
                    .form_dream_memory(agent_protocol::DreamMemoryParams {
                        session: context,
                        after_cursor: after.clone(),
                    })
                    .await?;
                let Some(formation) = result.formation else {
                    // No retained receipt remains after this cursor, so observing progress
                    // cannot hide paid usage from an interrupted formation.
                    if result.processed_until_seq > 0 {
                        self.session_store
                            .advance_dream_memory_cursor(
                                &record.id,
                                after.as_ref(),
                                &result.next_cursor,
                                Some(result.processed_until_seq),
                            )
                            .await?;
                    }
                    return Ok(false);
                };
                let call = formation.model_usage;
                let completed_at_ms = call.completed_at.checked_mul(1_000).ok_or_else(|| {
                    talos_error::TalosError::internal(
                        "Worker Dream usage timestamp exceeds supported range.",
                    )
                })?;
                let timezone = talos_config::parse_timezone(&call.timezone)?;
                let date = chrono::DateTime::from_timestamp_millis(completed_at_ms)
                    .ok_or_else(|| {
                        talos_error::TalosError::internal(
                            "Worker Dream usage timestamp is invalid.",
                        )
                    })?
                    .with_timezone(&timezone)
                    .date_naive()
                    .format("%Y-%m-%d")
                    .to_string();
                self.profiles
                    .record_usage(ProfileUsageUpdate::ModelCall(ModelCallUsageUpdate {
                        user_id: record.user_id.clone(),
                        session_id: Some(record.id.clone()),
                        request_id: call.request_id,
                        kind: ModelUsageKind::Dream,
                        model_ref: call.model_ref,
                        usage_available: call.usage.is_available(),
                        usage: call.usage,
                        timezone: call.timezone,
                        completed_at_ms,
                        usage_date: date,
                    }))
                    .await?;
                self.session_store
                    .advance_dream_memory_cursor(
                        &record.id,
                        after.as_ref(),
                        &result.next_cursor,
                        formation.covers_until_seq,
                    )
                    .await?;
                if let Some(error) = formation.failure {
                    return Err(talos_error::TalosError::from(error));
                }
                Ok(formation.covers_until_seq.is_some())
            })
            .await?;
        Ok(result.unwrap_or(false))
    }
}

#[cfg(test)]
#[path = "dream_memory_maintenance_tests.rs"]
mod tests;
