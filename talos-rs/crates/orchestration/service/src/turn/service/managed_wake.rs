use super::*;
use crate::agent_node::ManagedDispatch;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum WakeCandidate {
    Queue,
    Goal { explicit: bool },
}

impl TurnService {
    pub(super) async fn defer_managed_dispatch(
        &self,
        session: &SessionRecord,
        candidate: WakeCandidate,
    ) -> TalosResult<bool> {
        let Some(nodes) = self.nodes.as_ref() else {
            return Ok(false);
        };
        match nodes
            .wake_for_managed_dispatch(&session.user_id, &session.execution_node_id)
            .await?
        {
            ManagedDispatch::Proceed => return Ok(false),
            ManagedDispatch::Deferred => return Ok(true),
            ManagedDispatch::Waking => {}
        }
        let key = (session.id.clone(), candidate);
        if self
            .wake_dispatches
            .lock()
            .expect("Wake dispatch lock")
            .insert(key)
        {
            let service = self.clone();
            let session = session.clone();
            tokio::spawn(service.dispatch_after_wake(session, candidate));
        }
        Ok(true)
    }

    fn dispatch_after_wake(
        self,
        session: SessionRecord,
        candidate: WakeCandidate,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> {
        Box::pin(async move {
            let result = self
                .nodes
                .as_ref()
                .expect("managed Wake candidate has a node service")
                .wait_managed_dispatch(&session.user_id, &session.execution_node_id)
                .await;
            self.wake_dispatches
                .lock()
                .expect("Wake dispatch lock")
                .remove(&(session.id.clone(), candidate));
            if result.is_err() {
                return;
            }
            let result = match candidate {
                WakeCandidate::Queue => self.try_dispatch_next(&session.id).await.map(|_| ()),
                WakeCandidate::Goal { explicit } => self.continue_goal(session, explicit).await,
            };
            if let Err(error) = result {
                talos_logging::report_error(&error);
            }
        })
    }
}
