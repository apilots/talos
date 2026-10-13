//! Own execution task joins and recover failures without losing durable Turn state.

use super::*;

impl AgentExecutionDriver {
    pub(super) async fn launch_execution(
        &self,
        execution: AgentRunExecution,
        commit: AgentEventCommit,
        agent_id: String,
        purpose: ExecutionPurpose,
    ) -> TalosResult<oneshot::Receiver<TalosResult<AgentRunSettlement>>> {
        let AgentRunExecution {
            admission,
            events,
            settlement,
        } = execution;
        let identity = commit.active_turn.identity.clone();
        let last_sequence = commit
            .events
            .last()
            .expect("initial Agent admission contains canonical events")
            .sequence;
        let durable_tail = commit.active_turn.last_event_seq;
        let (completion_tx, completion_rx) = oneshot::channel();
        let (input_tx, input_rx) = mpsc::channel(1);
        let (terminal_commit_tx, _terminal_commit_rx) = watch::channel(false);
        let active = ActiveTimelineStream {
            identity: identity.clone(),
            agent_id,
            input_tx,
            terminal_commit_tx,
        };
        self.promote_admission(active.clone())?;
        self.event_service.publish_committed(commit.events);
        let driver = self.clone();
        let task_active = active.clone();
        let share_after_completion = matches!(purpose, ExecutionPurpose::Turn);
        let task = tokio::spawn(async move {
            match admission.permit.consume().await {
                Ok(()) => {
                    driver
                        .drive(
                            task_active,
                            events,
                            input_rx,
                            settlement,
                            (commit.active_turn.last_event_seq, last_sequence),
                            purpose,
                        )
                        .await
                }
                Err(error) => {
                    driver
                        .recover_failed_execution(&task_active, durable_tail, &error)
                        .await
                }
            }
        });
        let driver = self.clone();
        tokio::spawn(async move {
            let mut result = match task.await {
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
                        Ok(Some(current)) if current.identity == identity => {
                            driver
                                .recover_failed_execution(&active, current.last_event_seq, &error)
                                .await
                        }
                        Ok(_) => Err(error),
                        Err(lookup_error) => {
                            talos_logging::report_error(&error);
                            Err(lookup_error)
                        }
                    }
                }
            };
            if !matches!(result, Ok(AgentRunSettlement::WaitingForInput { .. })) {
                if let Err(error) = driver
                    .sessions
                    .commit_worker_history_cursor(
                        &identity,
                        agent_protocol::SessionStateVisibility::Local,
                    )
                    .await
                {
                    if let Err(original) = result {
                        talos_logging::report_error(&original);
                    }
                    result = Err(error);
                }
            }
            driver.remove_live_execution(&identity);
            if share_after_completion
                && !matches!(result, Ok(AgentRunSettlement::WaitingForInput { .. }))
            {
                driver
                    .sessions
                    .schedule_shared_history_cursor(identity.session_id.clone());
            }
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
