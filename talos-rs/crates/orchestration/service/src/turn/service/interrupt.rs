use super::*;
use talos_agent::AgentInterruptParams;
use talos_agent_api::{AgentEventPayload as CanonicalAgentEventPayload, AgentInterruptOutcome};

impl TurnService {
    pub(crate) async fn canonical_active_turn(
        &self,
        session_id: &str,
    ) -> TalosResult<Option<talos_store::session::AgentActiveTurnRecord>> {
        self.execution
            .execution_store
            .get_agent_active_turn(session_id)
            .await
    }

    pub(crate) async fn interrupt_canonical_turn(
        &self,
        session: &talos_store::session::SessionRecord,
        turn_id: &str,
    ) -> TalosResult<AgentInterruptOutcome> {
        let identity = AgentTurnRef::new(&session.id, turn_id);
        let Some(active) = self
            .execution
            .execution_store
            .get_agent_active_turn(&identity.session_id)
            .await?
        else {
            return self.completed_or_missing_interrupt(&identity).await;
        };
        if active.identity != identity {
            return Err(missing_active_turn(&identity));
        }
        let terminal_waiter = self.execution.terminal_commit_waiter(&identity);
        let recovery_owner = match &terminal_waiter {
            Some(_) => None,
            None => Some(
                self.execution
                    .begin_session_maintenance(&identity.session_id)?,
            ),
        };

        let cancellation = if active.state == ActiveTurnState::Cancelling {
            None
        } else {
            match self
                .execution
                .execution_store
                .begin_cancellation(&identity)
                .await
            {
                Ok(admission) => Some(admission),
                Err(error) if error.is_code(talos_error::ErrorCode::Conflict) => {
                    return self.completed_or_missing_interrupt(&identity).await;
                }
                Err(error) => return Err(error),
            }
        };
        let cleanup = match self
            .execution
            .coordinator
            .interrupt(AgentInterruptParams {
                agent_id: session.agent_id.clone(),
                identity: identity.clone(),
            })
            .await
        {
            Ok(outcome) => outcome,
            Err(error) if error.is_code(talos_error::ErrorCode::ResourceNotFound) => {
                AgentInterruptOutcome::Cancelled
            }
            Err(error) => {
                if let Some(cancellation) = cancellation {
                    if let Err(restore_error) = self
                        .execution
                        .execution_store
                        .restore_cancellation(cancellation)
                        .await
                    {
                        if self.has_terminal_fact(&identity).await? {
                            return Ok(AgentInterruptOutcome::Completed);
                        }
                        return Err(restore_error);
                    }
                }
                return Err(error);
            }
        };
        if matches!(cleanup, AgentInterruptOutcome::CleanupUnconfirmed { .. }) {
            return Ok(cleanup);
        }
        if let Some(waiter) = terminal_waiter {
            AgentExecutionDriver::wait_for_terminal_commit(waiter).await?;
            return Ok(cleanup);
        }
        self.complete_recovered_turn(
            &identity,
            talos_agent::AgentError::from(TalosError::new(
                talos_error::ErrorCode::Cancelled,
                "Agent Turn was interrupted",
            )),
            cleanup,
            recovery_owner.expect("orphan interrupt holds Session ownership"),
        )
        .await
    }

    /// Recover an orphan only while holding the existing Session exclusion.
    /// Service live absence is not proof of Harness cleanup; the exact control
    /// outcome must authorize terminal persistence before product completion.
    pub(super) async fn recover_terminal_turn(
        &self,
        session: &talos_store::session::SessionRecord,
        identity: &AgentTurnRef,
        failure: talos_agent::AgentError,
    ) -> TalosResult<AgentInterruptOutcome> {
        let ownership = self
            .execution
            .begin_session_maintenance(&identity.session_id)?;
        let cleanup = match self
            .execution
            .coordinator
            .interrupt(AgentInterruptParams {
                agent_id: session.agent_id.clone(),
                identity: identity.clone(),
            })
            .await
        {
            Ok(outcome) => outcome,
            Err(error) if error.is_code(talos_error::ErrorCode::ResourceNotFound) => {
                AgentInterruptOutcome::Cancelled
            }
            Err(error) => return Err(error),
        };
        self.complete_recovered_turn(identity, failure, cleanup, ownership)
            .await
    }

    async fn complete_recovered_turn(
        &self,
        identity: &AgentTurnRef,
        failure: talos_agent::AgentError,
        cleanup: AgentInterruptOutcome,
        ownership: crate::turn::agent_execution::SessionMaintenanceGuard,
    ) -> TalosResult<AgentInterruptOutcome> {
        if matches!(cleanup, AgentInterruptOutcome::CleanupUnconfirmed { .. }) {
            return Ok(cleanup);
        }
        let current = self.canonical_active_turn(&identity.session_id).await?;
        let Some(current) = current.filter(|active| active.identity == *identity) else {
            return self.completed_or_missing_interrupt(identity).await;
        };
        let settlement = self
            .execution
            .settle_recovered_turn(
                identity.clone(),
                current.last_event_seq,
                failure,
                cleanup.clone(),
            )
            .await?;
        drop(ownership);
        let AgentRunSettlement::Finished { event, result, .. } = settlement else {
            return Err(TalosError::internal(
                "terminal recovery returned a waiting settlement",
            ));
        };
        if let Err(error) = result {
            talos_logging::report_error(&error);
        }
        self.finish_turn(&event, None).await?;
        Ok(cleanup)
    }

    async fn completed_or_missing_interrupt(
        &self,
        identity: &AgentTurnRef,
    ) -> TalosResult<AgentInterruptOutcome> {
        if self.has_terminal_fact(identity).await? {
            Ok(AgentInterruptOutcome::Completed)
        } else {
            Err(missing_active_turn(identity))
        }
    }

    async fn has_terminal_fact(&self, identity: &AgentTurnRef) -> TalosResult<bool> {
        let tail = self
            .execution
            .execution_store
            .get_latest_agent_event_seq(&identity.session_id)
            .await?
            .unwrap_or_default();
        let mut terminal = false;
        crate::timeline::scan_turn_events(
            self.execution.execution_store.as_ref(),
            identity,
            tail,
            |event| {
                terminal = matches!(
                    event.payload,
                    CanonicalAgentEventPayload::TurnCompleted(_)
                        | CanonicalAgentEventPayload::TurnFailed(_)
                );
                Ok(())
            },
        )
        .await?;
        Ok(terminal)
    }
}

fn missing_active_turn(identity: &AgentTurnRef) -> TalosError {
    TalosError::new(
        talos_error::ErrorCode::ResourceNotFound,
        format!(
            "active turn `{}` for session `{}`",
            identity.turn_id, identity.session_id
        ),
    )
}
