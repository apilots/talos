//! Bounded-page scans of one Turn's canonical facts at a fixed durable tail.

use talos_agent_api::AgentTurnRef;
use talos_error::{TalosError, TalosResult};
use talos_store::session::{AgentEventRecord, AgentExecutionStore};

const TURN_EVENT_PAGE_SIZE: usize = 2048;

/// Visit complete Turn history without treating a page limit as a Turn limit.
/// Callers retain only their required projection; this does not collect a transcript.
pub(crate) async fn scan_turn_events(
    store: &dyn AgentExecutionStore,
    identity: &AgentTurnRef,
    through_event_seq: i64,
    mut visit: impl FnMut(AgentEventRecord) -> TalosResult<()> + Send,
) -> TalosResult<()> {
    let Some(first) = store.list_agent_turn_events(identity, 1).await?.pop() else {
        return Ok(());
    };
    let mut after = first.event_seq.saturating_sub(1);
    while after < through_event_seq {
        let events = store
            .list_agent_events_after(&identity.session_id, Some(after), TURN_EVENT_PAGE_SIZE)
            .await?;
        if events.is_empty() {
            return Err(TalosError::internal(
                "canonical Turn scan ended before its durable tail",
            ));
        }
        for event in events {
            if event.event_seq > through_event_seq {
                return Ok(());
            }
            after = event.event_seq;
            if event.identity == *identity {
                visit(event)?;
            }
        }
    }
    Ok(())
}
