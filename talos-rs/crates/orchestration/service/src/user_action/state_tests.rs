use super::*;
use talos_types::UserActionState;

#[test]
fn accepts_only_declared_user_action_transitions() {
    let allowed = [
        (UserActionState::Pending, UserActionState::ResponseAccepted),
        (UserActionState::Pending, UserActionState::Cancelled),
        (UserActionState::ResponseAccepted, UserActionState::Resuming),
        (
            UserActionState::ResponseAccepted,
            UserActionState::Cancelled,
        ),
        (UserActionState::Resuming, UserActionState::Resolved),
        (UserActionState::Resuming, UserActionState::Cancelled),
        (UserActionState::Resuming, UserActionState::ResponseAccepted),
        (UserActionState::Resuming, UserActionState::ResumeFailed),
        (UserActionState::Resuming, UserActionState::ExecutionUnknown),
    ];

    for (current, next) in allowed {
        assert_eq!(validate_transition(current, next), Ok(()));
    }
}

#[test]
fn rejects_every_undeclared_user_action_transition() {
    let states = [
        UserActionState::Pending,
        UserActionState::ResponseAccepted,
        UserActionState::Resuming,
        UserActionState::ResumeFailed,
        UserActionState::ExecutionUnknown,
        UserActionState::Resolved,
        UserActionState::Cancelled,
    ];
    let allowed = [
        (UserActionState::Pending, UserActionState::ResponseAccepted),
        (UserActionState::Pending, UserActionState::Cancelled),
        (UserActionState::ResponseAccepted, UserActionState::Resuming),
        (
            UserActionState::ResponseAccepted,
            UserActionState::Cancelled,
        ),
        (UserActionState::Resuming, UserActionState::Resolved),
        (UserActionState::Resuming, UserActionState::Cancelled),
        (UserActionState::Resuming, UserActionState::ResponseAccepted),
        (UserActionState::Resuming, UserActionState::ResumeFailed),
        (UserActionState::Resuming, UserActionState::ExecutionUnknown),
    ];

    for current in states {
        for next in states {
            if !allowed.contains(&(current, next)) {
                assert_eq!(
                    validate_transition(current, next),
                    Err(InvalidUserActionTransition { current, next })
                );
            }
        }
    }
}
