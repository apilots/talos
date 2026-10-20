use super::*;
use talos_service::{ConfigReloadPlan, ConfigService};

#[tokio::test]
async fn config_commit_is_excluded_until_the_actual_invocation_settles() {
    let gate = Arc::new(tokio::sync::RwLock::new(()));
    let config = ConfigService::new(
        Arc::new(talos_config::InMemoryLlmConfigStore::new(Default::default())),
        gate.clone(),
        Arc::new(|_| Ok(ConfigReloadPlan::new(|| {}))),
    );
    let (_events, events) = tokio::sync::mpsc::channel(1);
    let (settled, settlement) = tokio::sync::oneshot::channel();
    let execution = guard_execution(
        AgentRunExecution {
            admission: AgentAdmission {
                events: Vec::new(),
                permit: AgentAdmissionPermit::new(async { Ok(()) }),
            },
            events: AgentProgressEventReceiver::new(events),
            settlement: AgentRunSettlementReceiver::new(settlement),
        },
        gate.clone().read_owned().await,
        None,
    );
    execution.admission.permit.consume().await.unwrap();
    let error = config.reload_config(Default::default()).await.unwrap_err();
    assert!(error.is_code(talos_error::ErrorCode::Conflict));
    settled
        .send(Err(talos_error::TalosError::new(
            talos_error::ErrorCode::Cancelled,
            "Invocation was interrupted",
        )))
        .unwrap();
    assert!(execution
        .settlement
        .recv()
        .await
        .unwrap_err()
        .is_code(talos_error::ErrorCode::Cancelled));
    // Settlement receipt and guard release may be scheduled separately.
    let released = tokio::time::timeout(Duration::from_secs(1), gate.write_owned())
        .await
        .unwrap();
    drop(released);
    assert!(
        config
            .reload_config(Default::default())
            .await
            .unwrap()
            .success
    );
}
