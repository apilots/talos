use super::*;
#[test]
fn managed_worker_phases_round_trip_without_losing_directory_control_state() {
    use talos_types::ManagedWorkerPhase::*;
    for phase in [
        Provisioning,
        Starting,
        WaitingForRegistration,
        Ready,
        Sleeping,
        Stopped,
        Offline,
        Stopping,
        Resetting,
        Deleting,
        Failed,
    ] {
        let value = NodeInfo {
            node_id: "managed".into(),
            display_name: "Managed Talo".into(),
            mode: NodeMode::Remote,
            endpoint: None,
            status: NodeStatus::Enabled,
            health: NodeHealth::Unknown,
            lease_expires_at: None,
            last_heartbeat_at: None,
            created_at: 1,
            updated_at: 2,
            owner_user_id: Some("owner".into()),
            available: false,
            credential_expires_at: None,
            managed_worker_phase: Some(phase),
            system_info: None,
        };
        assert_eq!(decode_info(encode_info(value.clone())).unwrap(), value);
    }
    assert!(decode_managed_phase(0).is_err());
    assert!(decode_managed_phase(999).is_err());
}
