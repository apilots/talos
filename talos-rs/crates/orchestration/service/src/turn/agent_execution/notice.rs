//! Report secondary product failures without changing canonical execution outcomes.

use talos_agent_api::{AgentSystemNotice, AgentTurnRef, SystemNoticeContent, TimelineSystemNotice};
use talos_error::TalosError;

use super::AgentExecutionDriver;

impl AgentExecutionDriver {
    /// Completion and delivery owners report once, then publish the safe notice.
    /// Persistence failure is a separate diagnostic; never fabricate a durable cursor.
    pub(in crate::turn) async fn report_warning(&self, identity: &AgentTurnRef, error: TalosError) {
        talos_logging::report_error(&error);
        if let Err(persistence_error) = self
            .event_service
            .publish_notice(TimelineSystemNotice {
                identity: identity.clone(),
                occurred_at: chrono::Utc::now().timestamp(),
                notice: AgentSystemNotice {
                    level: talos_types::TalosTimelineNoticeLevel::Warning,
                    content: SystemNoticeContent::Error(error.to_api_error()),
                },
            })
            .await
        {
            talos_logging::report_error(&persistence_error.context("persist a product warning"));
        }
    }
}
