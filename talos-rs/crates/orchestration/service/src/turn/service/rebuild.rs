//! Repair only a product's derived execution view from retained Worker facts.
use super::{SessionRecord, TalosError, TalosResult, TurnService};
use agent_protocol::{AgentSessionEventCursor, AgentSessionEventsParams};
use talos_error::ErrorCode;
use talos_store::session::{WorkerProjectionParams, WorkerProjectionRebuildParams};

impl TurnService {
    /// The caller holds the node admission gate and product Session lease. This
    /// operation never clears Worker history or dispatches follow-up automation.
    pub(crate) async fn rebuild_session_view(&self, session: &SessionRecord) -> TalosResult<()> {
        let _maintenance = self.execution.begin_session_maintenance(&session.id)?;
        self.execution
            .sessions
            .ensure_execution_product_binding(session)
            .await?;
        let context = self
            .execution
            .sessions
            .build_agent_session_context(session)
            .await?;
        let source = self
            .execution
            .coordinator
            .read_session_execution_state(context.clone())
            .await?;
        let generation = self
            .execution
            .sessions
            .product_history_generation(&session.id)
            .await?;
        if source
            .as_ref()
            .map_or(0, |state| state.history_cursor.history_generation)
            != generation
        {
            return Err(TalosError::new(
                ErrorCode::EventCursorExpired,
                "Worker history changed; the product view cannot be rebuilt from this generation.",
            ));
        }
        let store = &self.execution.execution_store;
        let request_id = store
            .begin_worker_projection_rebuild(WorkerProjectionRebuildParams {
                session_id: session.id.clone(),
                execution_node_id: session.execution_node_id.clone(),
                history_generation: generation,
            })
            .await?;
        // An explicit zero cursor fences the original generation, including empty history.
        let mut cursor = AgentSessionEventCursor {
            session_id: session.id.clone(),
            generation,
            offset: 0,
        };
        loop {
            let page = self
                .execution
                .coordinator
                .read_session_events(AgentSessionEventsParams {
                    session: context.clone(),
                    after_cursor: Some(cursor.clone()),
                    limit: agent_protocol::MAX_EXECUTION_PAGE_EVENTS,
                })
                .await?;
            let more = page.has_more;
            let next = page.next_cursor.clone();
            if more && next.offset <= cursor.offset {
                return Err(TalosError::new(
                    ErrorCode::Conflict,
                    "Worker replay did not advance its source cursor.",
                ));
            }
            store
                .stage_worker_projection_rebuild(
                    &request_id,
                    WorkerProjectionParams {
                        session_id: session.id.clone(),
                        execution_node_id: session.execution_node_id.clone(),
                        after_cursor: Some(cursor),
                        page,
                    },
                )
                .await?;
            cursor = next;
            if !more {
                break;
            }
        }
        let final_source = self
            .execution
            .coordinator
            .read_session_execution_state(context)
            .await?;
        if final_source
            .as_ref()
            .map_or(0, |state| state.history_cursor.history_generation)
            != generation
        {
            return Err(TalosError::new(
                ErrorCode::EventCursorExpired,
                "Worker history changed while rebuilding the product view.",
            ));
        }
        store.finish_worker_projection_rebuild(&request_id).await?;
        self.settle_product_before_destruction(&session.id).await?;
        self.execution.publish_active_turn(&session.id).await?;
        Ok(())
    }
}
