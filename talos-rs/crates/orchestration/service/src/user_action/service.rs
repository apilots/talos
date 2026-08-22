use std::sync::Arc;

use talos_error::{TalosError, TalosResult};
use talos_store::session::{UserActionRecord, UserActionStore};
use talos_types::{QuestionAnswer, UserActionParams, UserActionResponse, UserActionState};

use super::state::validate_transition;

#[derive(Debug, Clone)]
pub(crate) struct AcceptedUserAction {
    pub(crate) record: UserActionRecord,
    pub(crate) should_resume: bool,
}

/// Owns typed UserAction validation and durable lifecycle transitions.
#[derive(Clone)]
pub(crate) struct UserActionService {
    store: Arc<dyn UserActionStore>,
}

impl UserActionService {
    pub(crate) fn new(store: Arc<dyn UserActionStore>) -> Self {
        Self { store }
    }

    pub(crate) async fn get(
        &self,
        session_id: &str,
        turn_id: &str,
        action_id: &str,
    ) -> TalosResult<Option<UserActionRecord>> {
        self.store.get(session_id, turn_id, action_id).await
    }

    pub(crate) async fn accept(
        &self,
        session_id: &str,
        turn_id: &str,
        action_id: &str,
        response: UserActionResponse,
    ) -> TalosResult<AcceptedUserAction> {
        let record = self
            .store
            .get(session_id, turn_id, action_id)
            .await?
            .ok_or_else(|| {
                TalosError::public(
                    talos_error::ErrorCode::ResourceNotFound,
                    format!("user action {action_id}"),
                )
            })?;
        validate_response(&record.action.params, &response)?;

        if let Some(existing) = &record.response {
            if existing != &response {
                return Err(TalosError::public(
                    talos_error::ErrorCode::Conflict,
                    format!("user action `{action_id}` already has a different response"),
                ));
            }
            return Ok(AcceptedUserAction {
                should_resume: false,
                record,
            });
        }
        if record.action.state != UserActionState::Pending {
            return Err(TalosError::public(
                talos_error::ErrorCode::Conflict,
                format!("user action `{action_id}` is not pending"),
            ));
        }
        match self
            .transition(
                record,
                UserActionState::ResponseAccepted,
                Some(response.clone()),
            )
            .await
        {
            Ok(accepted) => Ok(AcceptedUserAction {
                record: accepted,
                should_resume: true,
            }),
            Err(error) if error.is_code(talos_error::ErrorCode::Conflict) => {
                let current = self
                    .store
                    .get(session_id, turn_id, action_id)
                    .await?
                    .ok_or_else(|| {
                        TalosError::public(
                            talos_error::ErrorCode::ResourceNotFound,
                            format!("user action {action_id}"),
                        )
                    })?;
                if current.response.as_ref() == Some(&response) {
                    Ok(AcceptedUserAction {
                        record: current,
                        should_resume: false,
                    })
                } else {
                    Err(TalosError::public(
                        talos_error::ErrorCode::Conflict,
                        format!("user action `{action_id}` was answered concurrently"),
                    ))
                }
            }
            Err(error) => Err(error),
        }
    }

    pub(crate) async fn begin_resume(
        &self,
        record: UserActionRecord,
    ) -> TalosResult<UserActionRecord> {
        self.transition(record, UserActionState::Resuming, None)
            .await
    }

    pub(crate) async fn resolve(&self, record: UserActionRecord) -> TalosResult<UserActionRecord> {
        self.transition(record, UserActionState::Resolved, None)
            .await
    }

    pub(crate) async fn cancel(&self, record: UserActionRecord) -> TalosResult<UserActionRecord> {
        if record.action.state == UserActionState::Cancelled {
            return Ok(record);
        }
        self.transition(record, UserActionState::Cancelled, None)
            .await
    }

    pub(crate) async fn retry_or_fail(
        &self,
        record: UserActionRecord,
    ) -> TalosResult<UserActionRecord> {
        let next = if record.version <= 2 {
            UserActionState::ResponseAccepted
        } else {
            UserActionState::ResumeFailed
        };
        self.transition(record, next, None).await
    }

    pub(crate) async fn recover(
        &self,
        record: UserActionRecord,
    ) -> TalosResult<AcceptedUserAction> {
        match record.action.state {
            UserActionState::ResponseAccepted => Ok(AcceptedUserAction {
                record,
                should_resume: true,
            }),
            UserActionState::Resuming => Ok(AcceptedUserAction {
                record: self
                    .transition(record, UserActionState::ExecutionUnknown, None)
                    .await?,
                should_resume: false,
            }),
            UserActionState::Pending
            | UserActionState::ResumeFailed
            | UserActionState::ExecutionUnknown
            | UserActionState::Resolved
            | UserActionState::Cancelled => Ok(AcceptedUserAction {
                record,
                should_resume: false,
            }),
        }
    }

    async fn transition(
        &self,
        current: UserActionRecord,
        next_state: UserActionState,
        response: Option<UserActionResponse>,
    ) -> TalosResult<UserActionRecord> {
        validate_transition(current.action.state, next_state).map_err(|error| {
            TalosError::public(
                talos_error::ErrorCode::Conflict,
                format!(
                    "invalid user action transition {:?} -> {:?}",
                    error.current, error.next
                ),
            )
        })?;
        let mut next = current.clone();
        next.action.state = next_state;
        if let Some(response) = response {
            next.response = Some(response);
        }
        next.version = current.version.saturating_add(1);
        next.updated_at = chrono::Utc::now().timestamp_millis();
        self.store.compare_and_set(&current, next).await
    }
}

#[cfg(test)]
#[path = "service_tests.rs"]
mod tests;

fn validate_response(params: &UserActionParams, response: &UserActionResponse) -> TalosResult<()> {
    match (params, response) {
        (UserActionParams::DynamicToolResult(_), UserActionResponse::DynamicToolResult(_)) => {
            Ok(())
        }
        (UserActionParams::Question(params), UserActionResponse::Question(answer)) => {
            match answer {
                QuestionAnswer::SelectedOption { option_id } => {
                    if !params.options.iter().any(|option| option.id == *option_id) {
                        return Err(TalosError::public(
                            talos_error::ErrorCode::InvalidRequest,
                            format!("question option `{option_id}` does not exist"),
                        ));
                    }
                }
                QuestionAnswer::FreeForm { text } => {
                    if text.trim().is_empty() || text.len() > 4096 {
                        return Err(TalosError::public(
                            talos_error::ErrorCode::InvalidRequest,
                            "question free-form answer must contain 1 to 4096 bytes",
                        ));
                    }
                }
            }
            Ok(())
        }
        (
            UserActionParams::SandboxApproval(params),
            UserActionResponse::SandboxApproval(decision),
        ) => {
            if !params.available_decisions.contains(decision) {
                return Err(TalosError::public(
                    talos_error::ErrorCode::InvalidRequest,
                    format!("sandbox approval decision `{decision:?}` is not available"),
                ));
            }
            Ok(())
        }
        _ => Err(TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            "user action response variant does not match its parameters",
        )),
    }
}
