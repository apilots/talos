//! Own execution task joins and recover failures without losing durable Turn state.

use super::*;

impl AgentExecutionDriver {
    pub(super) async fn launch_execution(
        &self,
        execution: AgentRunExecution,
        session: agent_protocol::AgentSessionContext,
        purpose: ExecutionPurpose,
    ) -> TalosResult<oneshot::Receiver<TalosResult<AgentRunSettlement>>> {
        let AgentRunExecution {
            admission,
            events,
            settlement,
        } = execution;
        let identity = admission
            .events
            .first()
            .ok_or_else(|| TalosError::internal("Worker admission is empty."))?
            .identity
            .clone();
        let last_sequence = admission
            .events
            .last()
            .expect("Worker admission is nonempty")
            .sequence;
        let durable_tail = 0;
        let (completion_tx, completion_rx) = oneshot::channel();
        let (input_tx, input_rx) = mpsc::channel(1);
        let (terminal_commit_tx, _terminal_commit_rx) = watch::channel(false);
        let active = ActiveTimelineStream {
            identity: identity.clone(),
            session,
            input_tx,
            terminal_commit_tx,
        };
        self.promote_admission(active.clone())?;
        let source = self
            .sessions
            .load_record(&identity.session_id)
            .await?
            .ok_or_else(|| TalosError::internal("Product execution source is missing."))?;
        let observation_stop = self.observation_stop(&source.execution_node_id);
        let driver = self.clone();
        let task_active = active.clone();
        let task = tokio::spawn(async move {
            match admission.permit.consume().await {
                Ok(()) => {
                    driver
                        .drive(
                            task_active,
                            events,
                            input_rx,
                            settlement,
                            (durable_tail, last_sequence),
                            observation_stop,
                        )
                        .await
                }
                Err(error) => driver
                    .recover_failed_execution(&task_active, durable_tail, &error)
                    .await
                    .map(Some),
            }
        });
        let task = tokio_util::task::AbortOnDropHandle::new(task);
        let driver = self.clone();
        self.spawn_delivery(async move {
            let result = match task.await {
                Ok(result) => result,
                Err(source) => {
                    let error = TalosError::internal_with_source(
                        "The Agent execution task stopped unexpectedly.",
                        source,
                    );
                    match driver
                        .execution_store
                        .get_agent_active_turn(&identity.session_id)
                        .await
                    {
                        Ok(Some(current)) if current.identity == identity => driver
                            .recover_failed_execution(&active, current.last_event_seq, &error)
                            .await
                            .map(Some),
                        Ok(_) => Err(error),
                        Err(lookup_error) => {
                            talos_logging::report_error(&error);
                            Err(lookup_error)
                        }
                    }
                }
            };
            let mut result = match result {
                Ok(Some(settlement)) => Ok(settlement),
                Ok(None) => {
                    driver.remove_live_execution(&identity);
                    return;
                }
                Err(error) => Err(error),
            };
            if let Ok(settlement) = &result {
                if let Err(error) = driver
                    .finish_product_delivery(&active, settlement, &purpose)
                    .await
                {
                    result = Err(error);
                }
            }
            driver.remove_live_execution(&identity);
            // A cancelled receiver no longer owns reporting; committed facts remain intact.
            if let Err(result) = completion_tx.send(result) {
                match result {
                    Err(error)
                    | Ok(AgentRunSettlement::Finished {
                        result: Err(error), ..
                    }) => talos_logging::report_error(&error),
                    Ok(_) => {}
                }
            }
        });
        Ok(completion_rx)
    }
}
