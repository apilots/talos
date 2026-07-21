use super::*;

#[test]
fn capability_unsupported_maps_to_product_validation_error() {
    let error = AgentError::capability_unsupported("agent-a", "streaming");

    assert_eq!(error.kind, AgentErrorKind::CapabilityUnsupported);
    assert_eq!(
        error.into_talos_error().to_string(),
        "Validation error: agent `agent-a` does not support capability `streaming`"
    );
}

#[test]
fn run_not_found_maps_to_product_resource_not_found() {
    let error = AgentError::run_not_found("active turn for session s1");

    assert_eq!(error.kind, AgentErrorKind::RunNotFound);
    assert!(matches!(
        error.into_talos_error(),
        TalosError::ResourceNotFound(_)
    ));
}

#[test]
fn runtime_error_maps_to_product_internal_error() {
    let error = AgentError::runtime_error("backend failed");

    assert_eq!(error.kind, AgentErrorKind::RuntimeError);
    assert!(matches!(
        error.into_talos_error(),
        TalosError::InternalError(_)
    ));
}

#[test]
fn talos_error_maps_to_agent_error_kind_by_boundary_context() {
    assert_eq!(
        AgentError::from(TalosError::ResourceNotFound("missing run".to_string())).kind,
        AgentErrorKind::RunNotFound
    );
    assert_eq!(
        AgentError::from(TalosError::Conflict("active".to_string())).kind,
        AgentErrorKind::RunConflict
    );
    assert_eq!(
        AgentError::from(TalosError::Cancelled("user interrupted".to_string())).kind,
        AgentErrorKind::Interrupted
    );
}

#[test]
fn active_turn_running_round_trips_without_losing_busy_semantics() {
    let error = AgentError::from(TalosError::ActiveTurnRunning(
        "session `session-1` already has an active turn".to_string(),
    ));

    assert_eq!(error.kind, AgentErrorKind::ActiveTurnRunning);
    assert!(matches!(
        error.into_talos_error(),
        TalosError::ActiveTurnRunning(message)
            if message == "session `session-1` already has an active turn"
    ));
}
