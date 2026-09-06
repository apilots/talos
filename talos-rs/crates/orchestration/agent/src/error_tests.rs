use super::*;

#[test]
fn capability_unsupported_maps_to_product_validation_error() {
    let error = AgentError::capability_unsupported("agent-a", "streaming");

    assert_eq!(error.kind, AgentErrorKind::CapabilityUnsupported);
    assert_eq!(
        error.into_talos_error().to_string(),
        "InvalidRequest: agent `agent-a` does not support capability `streaming`"
    );
}

#[test]
fn run_not_found_maps_to_product_resource_not_found() {
    let error = AgentError::run_not_found("active turn for session s1");

    assert_eq!(error.kind, AgentErrorKind::RunNotFound);
    assert!(error
        .into_talos_error()
        .is_code(talos_error::ErrorCode::ResourceNotFound));
}

#[test]
fn provider_error_maps_to_product_internal_error() {
    let error = AgentError::provider_error("backend failed");

    assert_eq!(error.kind, AgentErrorKind::ProviderError);
    assert!(error.into_talos_error().is_internal());
}

#[test]
fn talos_error_maps_to_agent_error_kind_by_boundary_context() {
    assert_eq!(
        AgentError::from(TalosError::public(
            talos_error::ErrorCode::ResourceNotFound,
            "missing run".to_string()
        ))
        .kind,
        AgentErrorKind::RunNotFound
    );
    assert_eq!(
        AgentError::from(TalosError::public(
            talos_error::ErrorCode::Conflict,
            "active".to_string()
        ))
        .kind,
        AgentErrorKind::RunConflict
    );
    assert_eq!(
        AgentError::from(TalosError::public(
            talos_error::ErrorCode::Cancelled,
            "user interrupted".to_string()
        ))
        .kind,
        AgentErrorKind::Interrupted
    );
}

#[test]
fn active_turn_running_round_trips_without_losing_busy_semantics() {
    let error = AgentError::from(TalosError::public(
        talos_error::ErrorCode::ActiveTurnRunning,
        "session `session-1` already has an active turn".to_string(),
    ));

    assert_eq!(error.kind, AgentErrorKind::ActiveTurnRunning);
    let product_error = error.into_talos_error();
    assert!(product_error.is_code(talos_error::ErrorCode::ActiveTurnRunning));
    assert_eq!(
        product_error.diagnostic_message(),
        "session `session-1` already has an active turn"
    );
}

#[test]
fn file_input_error_is_non_retryable_and_keeps_the_public_run_code() {
    let error = AgentError::from(TalosError::file_error(
        talos_error::TalosFileErrorKind::UnsupportedAttachment,
        "attachments are unsupported",
    ));

    assert_eq!(
        error.kind,
        AgentErrorKind::RunFailure(talos_error::ErrorCode::FileInputRejected)
    );
    assert!(!error.retryable);
}
