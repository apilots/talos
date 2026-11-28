use crate::AgentEventReducer;
use agent_protocol::*;
use std::collections::BTreeMap;
use talos_agent_api::{
    AgentEvent, AgentEventPayload, AgentMessageBlockStatus, AgentMessageStatus, AgentTurnFailed,
    AgentTurnRef,
};
use talos_error::{ErrorCode, TalosError, TalosResult};

use super::{InputCommit, LocalExecutionHarness, RecordedInvocation};

impl LocalExecutionHarness {
    pub(super) async fn drive(
        &self,
        invocation: RecordedInvocation,
    ) -> TalosResult<AgentRunSettlement> {
        let session = invocation.session.clone();
        let identity = invocation.identity.clone();
        match self.drive_events(invocation).await {
            Ok(result) => Ok(result),
            Err(error) => {
                match self
                    .inner
                    .interrupt(AgentInterruptParams {
                        agent_id: session.agent_id.clone(),
                        identity: identity.clone(),
                    })
                    .await
                {
                    Ok(talos_agent_api::AgentInterruptOutcome::CleanupUnconfirmed { .. }) => {
                        return Err(TalosError::new(
                            ErrorCode::RunCleanupUnconfirmed,
                            "Worker execution cleanup is unconfirmed.",
                        ))
                    }
                    Err(cleanup) if !cleanup.is_code(ErrorCode::ResourceNotFound) => {
                        talos_logging::report_error(&error);
                        return Err(cleanup);
                    }
                    _ => {}
                }
                self.fail_execution(&session, &identity, error).await
            }
        }
    }

    pub(super) async fn fail_execution(
        &self,
        session: &AgentSessionContext,
        identity: &AgentTurnRef,
        error: TalosError,
    ) -> TalosResult<AgentRunSettlement> {
        let checkpoint = self
            .store
            .checkpoint(identity)
            .await?
            .ok_or_else(|| TalosError::internal("Worker failure checkpoint is missing."))?;
        let mut reducer = AgentEventReducer::from_checkpoint(identity.clone(), checkpoint)?;
        let mut closed = reducer.close_open_lifecycle(
            AgentMessageStatus::Failed,
            AgentMessageBlockStatus::Failed,
            chrono_now(),
        )?;
        let failed = AgentEvent {
            identity: identity.clone(),
            sequence: reducer.next_sequence(),
            occurred_at: chrono_now(),
            payload: AgentEventPayload::TurnFailed(AgentTurnFailed {
                error: error.to_api_error(),
                retryable: false,
            }),
        };
        reducer.apply(failed.clone())?;
        closed.push(failed.clone());
        let metrics = reducer.checkpoint().metrics.clone();
        self.store
            .append(
                session,
                closed,
                &reducer.checkpoint(),
                Some(AgentExecutionSettlement::Finished {
                    memory_usage: Vec::new(),
                    metrics: metrics.clone(),
                }),
            )
            .await?;
        Ok(AgentRunSettlement::Finished {
            event: failed,
            metrics,
            result: Err(error),
        })
    }

    async fn drive_events(
        &self,
        invocation: RecordedInvocation,
    ) -> TalosResult<AgentRunSettlement> {
        let RecordedInvocation {
            session,
            context,
            mut reducer,
            mut events,
            settlement,
            mut inputs,
            delivery,
            ..
        } = invocation;
        let session = &session;
        let mut pending: Option<InputCommit> = None;
        let mut ordered = BTreeMap::<u64, AgentEvent>::new();
        let mut batch = Vec::with_capacity(64);
        let mut deliveries = tokio::task::JoinSet::new();
        let mut inputs_open = true;
        loop {
            if let Some(input) = pending.take() {
                let next = input
                    .admission
                    .events
                    .first()
                    .ok_or_else(|| TalosError::internal("Worker input admission is empty."))?
                    .sequence;
                if next == reducer.next_sequence() {
                    let mut candidate = reducer.clone();
                    for event in &input.admission.events {
                        candidate.apply(event.clone())?;
                    }
                    if let Err(error) = self
                        .store
                        .admit(
                            session,
                            &input.command,
                            input.admission.events.clone(),
                            &candidate.checkpoint(),
                        )
                        .await
                    {
                        let _ = input.result.send(Err(error.clone()));
                        return Err(error);
                    }
                    reducer = candidate;
                    let _ = input.result.send(Ok(()));
                    deliveries.spawn(async move { input.admission.permit.consume().await });
                } else if next > reducer.next_sequence() {
                    pending = Some(input);
                } else {
                    let _ = input.result.send(Err(TalosError::new(
                        ErrorCode::Conflict,
                        "Worker input no longer follows the committed execution tail.",
                    )));
                }
            }
            let mut committed = Vec::new();
            let mut candidate = reducer.clone();
            while let Some(event) = ordered.remove(&candidate.next_sequence()) {
                candidate.apply(event.clone())?;
                committed.push(event);
            }
            if !committed.is_empty() {
                self.store
                    .append(session, committed.clone(), &candidate.checkpoint(), None)
                    .await?;
                reducer = candidate;
                for event in committed {
                    let _ = delivery.send(event).await;
                }
                continue;
            }
            tokio::select! {
                count = events.recv_many(&mut batch, 64) => {
                    if count == 0 { break; }
                    for event in batch.drain(..) {
                        if event.sequence < reducer.next_sequence() || ordered.insert(event.sequence, event).is_some() { return Err(TalosError::new(ErrorCode::Conflict, "Worker received duplicate execution sequence.")); }
                    }
                    if ordered.len() > 256 { return Err(TalosError::new(ErrorCode::Conflict, "Worker execution sequence gap exceeded its bounded buffer.")); }
                }
                input = inputs.recv(), if inputs_open && pending.is_none() => {
                    match input { Some(input) => pending = Some(input), None => inputs_open = false }
                }
                result = deliveries.join_next(), if !deliveries.is_empty() => {
                    match result { Some(Ok(result)) => result?, Some(Err(error)) => return Err(TalosError::internal_with_source("Worker input delivery task failed.", error)), None => {} }
                }
            }
        }
        if let Some(input) = pending {
            let _ = input.result.send(Err(TalosError::new(
                ErrorCode::Conflict,
                "Worker settled before input became durable.",
            )));
        }
        if !ordered.is_empty() {
            return Err(TalosError::new(
                ErrorCode::Conflict,
                "Worker execution ended with a sequence gap.",
            ));
        }
        while let Some(result) = deliveries.join_next().await {
            result.map_err(|error| {
                TalosError::internal_with_source("Worker input delivery task failed.", error)
            })??;
        }
        let result = settlement.recv().await?;
        let (event, persisted) = match &result {
            AgentRunSettlement::WaitingForInput { event, metrics } => (
                event.clone(),
                AgentExecutionSettlement::WaitingForInput {
                    metrics: metrics.clone(),
                },
            ),
            AgentRunSettlement::Finished { event, metrics, .. } => (
                event.clone(),
                AgentExecutionSettlement::Finished {
                    memory_usage: match (&self.terminal_observer, context) {
                        (Some(observer), Some(context)) => observer(context, event.clone()).await,
                        _ => Vec::new(),
                    },
                    metrics: metrics.clone(),
                },
            ),
        };
        reducer.apply(event.clone())?;
        self.store
            .append(session, vec![event], &reducer.checkpoint(), Some(persisted))
            .await?;
        Ok(result)
    }
}

fn chrono_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock follows epoch")
        .as_secs() as i64
}
