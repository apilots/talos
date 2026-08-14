use talos_types::UserActionState;

/// Error returned when a UserAction state edge is not part of the domain lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidUserActionTransition {
    pub current: UserActionState,
    pub next: UserActionState,
}

/// Validates one explicit UserAction lifecycle edge without performing I/O.
pub fn validate_transition(
    current: UserActionState,
    next: UserActionState,
) -> Result<(), InvalidUserActionTransition> {
    match (current, next) {
        (UserActionState::Pending, UserActionState::ResponseAccepted)
        | (UserActionState::Pending, UserActionState::Cancelled)
        | (UserActionState::ResponseAccepted, UserActionState::Resuming)
        | (UserActionState::ResponseAccepted, UserActionState::Cancelled)
        | (UserActionState::Resuming, UserActionState::Resolved)
        | (UserActionState::Resuming, UserActionState::Cancelled)
        | (UserActionState::Resuming, UserActionState::ResponseAccepted)
        | (UserActionState::Resuming, UserActionState::ResumeFailed)
        | (UserActionState::Resuming, UserActionState::ExecutionUnknown) => Ok(()),
        (current, next) => Err(InvalidUserActionTransition { current, next }),
    }
}

#[cfg(test)]
#[path = "state_tests.rs"]
mod tests;
