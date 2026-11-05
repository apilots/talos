//! Short-lived authority for an explicit request delayed by the initial grace.
//! This is not queued work and is deliberately not recovered after restart.
use super::*;

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct CancelledDispatchBoundary {
    identity: AgentTurnRef,
    event_seq: i64,
}

pub(super) struct ExplicitDispatch {
    object: AutomationObjectRef,
    boundary: CancelledDispatchBoundary,
}

pub(crate) fn automation_object(origin: &AutomationTurnOrigin) -> AutomationObjectRef {
    match origin {
        AutomationTurnOrigin::Goal { goal_id, .. } => AutomationObjectRef::Goal {
            goal_id: goal_id.clone(),
        },
        AutomationTurnOrigin::Loop { loop_id, .. } => AutomationObjectRef::Loop {
            loop_id: loop_id.clone(),
        },
    }
}

impl SessionAutomationService {
    /// Capture before applying the User command: a later cancellation must win.
    pub(crate) async fn cancelled_dispatch_boundary(
        &self,
        session_id: &str,
    ) -> TalosResult<Option<CancelledDispatchBoundary>> {
        Ok(self
            .execution
            .get_latest_agent_terminal_event(session_id)
            .await?
            .filter(|event| {
                matches!(&event.payload, AgentEventPayload::TurnFailed(failed) if failed.error.code == ErrorCode::Cancelled)
            })
            .map(|event| CancelledDispatchBoundary {
                identity: event.identity,
                event_seq: event.event_seq,
            }))
    }

    pub(crate) fn allow_explicit_dispatch(
        &self,
        session_id: &str,
        object: AutomationObjectRef,
        boundary: Option<CancelledDispatchBoundary>,
    ) {
        let Some(boundary) = boundary else { return };
        let mut sessions = self.explicit_dispatch.lock().expect("explicit dispatch");
        let entries = sessions.entry(session_id.to_owned()).or_default();
        entries.retain(|entry| entry.object != object);
        entries.push(ExplicitDispatch { object, boundary });
    }

    pub(crate) async fn allows_explicit_dispatch(
        &self,
        session_id: &str,
        object: &AutomationObjectRef,
    ) -> TalosResult<bool> {
        if self
            .execution
            .get_agent_active_turn(session_id)
            .await?
            .is_some()
        {
            return Ok(false);
        }
        let boundary = self.cancelled_dispatch_boundary(session_id).await?;
        let mut sessions = self.explicit_dispatch.lock().expect("explicit dispatch");
        let Some(boundary) = boundary else {
            sessions.remove(session_id);
            return Ok(false);
        };
        let Some(entries) = sessions.get_mut(session_id) else {
            return Ok(false);
        };
        entries.retain(|entry| entry.boundary == boundary);
        let allowed = entries.iter().any(|entry| &entry.object == object);
        if entries.is_empty() {
            sessions.remove(session_id);
        }
        Ok(allowed)
    }

    pub(crate) fn forget_explicit_dispatch(&self, session_id: &str, object: &AutomationObjectRef) {
        let mut sessions = self.explicit_dispatch.lock().expect("explicit dispatch");
        if let Some(entries) = sessions.get_mut(session_id) {
            entries.retain(|entry| &entry.object != object);
            if entries.is_empty() {
                sessions.remove(session_id);
            }
        }
    }

    pub(crate) fn forget_session_dispatch(&self, session_id: &str) {
        self.explicit_dispatch
            .lock()
            .expect("explicit dispatch")
            .remove(session_id);
    }
}
