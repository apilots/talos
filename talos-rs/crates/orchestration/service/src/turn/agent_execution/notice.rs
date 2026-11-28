//! Report secondary product failures without changing canonical execution outcomes.

use talos_agent_api::{AgentSystemNotice, AgentTurnRef, SystemNoticeContent, TimelineSystemNotice};
use talos_error::TalosError;

use super::AgentExecutionDriver;

impl AgentExecutionDriver {
    pub(in crate::turn) async fn complete_worker_settlement_with_notice(
        &self,
        cursor: &agent_protocol::AgentSessionEventCursor,
        notice: TimelineSystemNotice,
    ) -> talos_error::TalosResult<bool> {
        self.event_service
            .complete_worker_settlement_with_notice(cursor, notice)
            .await
    }

    /// Completion and delivery owners report once, then publish the safe notice.
    /// Persistence failure is a separate diagnostic; never fabricate a durable cursor.
    pub(in crate::turn) async fn report_warning(&self, identity: &AgentTurnRef, error: TalosError) {
        talos_logging::report_error(&error);
        self.publish_warning(identity, error.to_api_error()).await;
    }

    /// Worker execution already reported its typed source; product only publishes its safe notice.
    pub(in crate::turn) async fn publish_warning(
        &self,
        identity: &AgentTurnRef,
        error: talos_error::ApiError,
    ) {
        if let Err(persistence_error) = self
            .event_service
            .publish_notice(TimelineSystemNotice {
                identity: identity.clone(),
                occurred_at: chrono::Utc::now().timestamp(),
                notice: AgentSystemNotice {
                    level: talos_types::TalosTimelineNoticeLevel::Warning,
                    content: SystemNoticeContent::Error(error),
                },
            })
            .await
        {
            talos_logging::report_error(&persistence_error.context("persist a product warning"));
        }
    }
}
