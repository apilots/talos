//! Live delivery only wakes the single cursor-based product projection path.
use super::{ActiveTimelineStream, AgentExecutionDriver, ExecutionPurpose, RunningInputCommit};
use talos_agent::{AgentProgressEventReceiver, AgentRunSettlement, AgentRunSettlementReceiver};
use talos_agent_api::AgentEventPayload;
use talos_error::{ErrorCode, TalosError, TalosResult};
use tokio::sync::mpsc;

impl AgentExecutionDriver {
    pub(super) async fn drive(
        &self,
        active: ActiveTimelineStream,
        mut events: AgentProgressEventReceiver,
        mut inputs: mpsc::Receiver<RunningInputCommit>,
        settlement: AgentRunSettlementReceiver,
        _tail: (i64, u64),
        purpose: ExecutionPurpose,
    ) -> TalosResult<AgentRunSettlement> {
        if let Err(error) = self.project_worker(&active.session).await {
            return self.recover_failed_execution(&active, 0, &error).await;
        }
        let mut batch = Vec::with_capacity(64);
        let mut inputs_open = true;
        let mut deliveries = tokio::task::JoinSet::new();
        loop {
            tokio::select! {
                count = events.recv_many(&mut batch, 64) => {
                    batch.clear();
                    if count == 0 { break; }
                    if let Err(error) = self.project_worker(&active.session).await { return self.recover_failed_execution(&active, 0, &error).await; }
                }
                input = inputs.recv(), if inputs_open => match input {
                    Some(mut input) => {
                        let driver = self.clone();
                        let session = active.session.clone();
                        let identity = active.identity.clone();
                        deliveries.spawn(async move {
                            let result = async {
                                input.permit.consume().await?;
                                if let Err(error)=driver.project_worker(&session).await {driver.report_warning(&identity,error).await;}
                                if let Some(queued) = input.queued_input.take() {
                                    if let Err(error) = queued.commit() { driver.report_warning(&identity, error.context("remove a committed queued input")).await; }
                                }
                                Ok(())
                            }.await;
                            let _ = input.result_tx.send(result.clone());
                            if result.as_ref().err().is_some_and(agent_protocol::AgentAdmissionUnconfirmed::contains) {Ok(())} else {result}
                        });
                    }
                    None => inputs_open = false,
                },
                result = deliveries.join_next(), if !deliveries.is_empty() => {
                    if let Some(result) = result { result.map_err(|error| TalosError::internal_with_source("Product input projection task failed.", error))??; }
                }
            }
        }
        while let Some(result) = deliveries.join_next().await {
            result.map_err(|error| {
                TalosError::internal_with_source("Product input projection task failed.", error)
            })??;
        }
        let settlement = match settlement.recv().await {
            Ok(settlement) => settlement,
            Err(error) => return self.recover_failed_execution(&active, 0, &error).await,
        };
        self.project_worker(&active.session).await?;
        if let AgentRunSettlement::Finished { event, .. } = &settlement {
            if !purpose.is_compaction()
                && matches!(event.payload, AgentEventPayload::TurnCompleted(_))
            {
                let record = self
                    .execution_store
                    .get_agent_event_by_sequence(&event.identity, event.sequence)
                    .await?
                    .ok_or_else(|| {
                        TalosError::internal("Projected Worker terminal event is missing.")
                    })?;
                let checkpoint = self
                    .event_service
                    .turn_checkpoint(&event.identity, record.event_seq)
                    .await?;
                if let Some(model) = &self.inner_model {
                    if let Err(error) = self
                        .sessions
                        .generate_title(model, event, &checkpoint.messages)
                        .await
                    {
                        self.report_warning(
                            &event.identity,
                            error.context("generate Session title"),
                        )
                        .await;
                    }
                }
            }
            let _ = active.terminal_commit_tx.send(true);
        } else if purpose.is_compaction() {
            return Err(TalosError::new(
                ErrorCode::ContextCompactionFailed,
                "Agent compaction cannot wait for User input.",
            ));
        }
        Ok(settlement)
    }

    /// A product/transport failure does not interrupt an admitted Worker or
    /// manufacture its terminal outcome. Recovery observes its durable facts.
    pub(super) async fn recover_failed_execution(
        &self,
        active: &ActiveTimelineStream,
        _expected_event_seq: i64,
        error: &TalosError,
    ) -> TalosResult<AgentRunSettlement> {
        let settlement = self
            .recovered_worker_settlement(&active.identity, error)
            .await?;
        if matches!(settlement, AgentRunSettlement::Finished { .. }) {
            let _ = active.terminal_commit_tx.send(true);
        }
        Ok(settlement)
    }
}
