use super::*;
use crate::AgentNodeTransport;

#[test]
fn identity_survives_restart_and_preserves_other_configuration() {
    let home = tempfile::tempdir().unwrap();
    let conf = home.path().join("conf");
    std::fs::create_dir_all(&conf).unwrap();
    let path = conf.join("talos.yaml");
    std::fs::write(
        &path,
        "timezone: Asia/Shanghai\ncloud:\n  agent_base_url: https://agent.talos.love\n",
    )
    .unwrap();
    let store = YamlConfigStore::new(path.clone());
    let runner = AgentNodeRunnerConfig {
        transport: Some(AgentNodeTransport::Outbound),
        node_id: "a03f484f-dfb3-4bf8-8796-35c6ce239d31".into(),
        control_plane_endpoint: "https://agent.talos.love".into(),
        credential_file: conf.join("worker.credential"),
        trusted_ca_file: None,
    };
    store.begin_node_enrollment(None).unwrap();
    assert!(store.begin_node_enrollment(None).is_err());
    store
        .save_node_identity(&runner, "PRIVATE_NODE_TOKEN")
        .unwrap();
    let reopened = YamlConfigStore::new(path.clone());
    assert_eq!(
        reopened.load_agent_node().unwrap().runner.unwrap().node_id,
        runner.node_id
    );
    assert!(!reopened.node_enrollment_pending().unwrap());
    assert!(!home.path().join(".worker-state.json").exists());
    assert!(!home.path().join(".worker-state.lock").exists());
    assert!(reopened.begin_node_enrollment(None).is_err());
    let yaml = std::fs::read_to_string(path).unwrap();
    assert!(yaml.contains("Asia/Shanghai"));
    assert!(!yaml.contains("PRIVATE_NODE_TOKEN"));
    assert_eq!(
        std::fs::read_to_string(&runner.credential_file).unwrap(),
        "PRIVATE_NODE_TOKEN"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&runner.credential_file)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        // Once recovery has completed, normal startup must not need to create
        // another lock in a mounted read-only configuration directory.
        std::fs::remove_file(reopened.lock_path()).unwrap();
        std::fs::set_permissions(&conf, std::fs::Permissions::from_mode(0o500)).unwrap();
        let registered = reopened.complete_node_enrollment(&runner.node_id, "PRIVATE_NODE_TOKEN");
        std::fs::set_permissions(&conf, std::fs::Permissions::from_mode(0o700)).unwrap();
        registered.unwrap();
    }
}

#[test]
fn failed_credential_save_keeps_uuid_for_recovery() {
    let home = tempfile::tempdir().unwrap();
    let conf = home.path().join("conf");
    std::fs::create_dir_all(&conf).unwrap();
    let path = conf.join("talos.yaml");
    std::fs::write(&path, "{}").unwrap();
    let store = YamlConfigStore::new(path);
    let runner: AgentNodeRunnerConfig = serde_yaml::from_str(&format!(
        "node_id: stable-uuid\ncontrol_plane_endpoint: https://agent.talos.love\ncredential_file: {}\n", conf.join("credential").display())).unwrap();
    std::fs::create_dir(&runner.credential_file).unwrap();
    assert!(store.save_node_identity(&runner, "TOKEN").is_err());
    let reopened = YamlConfigStore::new(conf.join("talos.yaml"));
    assert!(reopened.node_enrollment_pending().unwrap());
    assert!(reopened.begin_node_enrollment(None).is_err());
    assert_eq!(
        store.load_agent_node().unwrap().runner.unwrap().node_id,
        "stable-uuid"
    );
    std::fs::remove_dir(&runner.credential_file).unwrap();
    reopened
        .save_node_identity(&runner, "RECOVERED_TOKEN")
        .unwrap();
    assert!(!reopened.node_enrollment_pending().unwrap());
    assert!(!home.path().join(".worker-state.json").exists());
    assert!(!home.path().join(".worker-state.lock").exists());
    assert_eq!(
        std::fs::read_to_string(&runner.credential_file).unwrap(),
        "RECOVERED_TOKEN"
    );
}

#[test]
fn sync_opt_out_survives_restart_without_changing_worker_or_endpoints() {
    let home = tempfile::tempdir().unwrap();
    let path = home.path().join("talos.yaml");
    std::fs::write(&path, "timezone: Asia/Shanghai\nsync:\n  hub_endpoint: wss://127.0.0.1:9220/v1/sync\ncloud:\n  agent_base_url: https://127.0.0.1:9210\n").unwrap();
    let store = YamlConfigStore::new(path.clone());
    assert_eq!(store.load_sync().unwrap().enabled, None);
    store.save_sync_enabled(false).unwrap();
    let reopened = YamlConfigStore::new(path.clone());
    assert_eq!(reopened.load_sync().unwrap().enabled, Some(false));
    assert_eq!(
        reopened.load_sync().unwrap().hub_endpoint,
        "wss://127.0.0.1:9220/v1/sync"
    );
    reopened.save_sync_enabled(true).unwrap();
    let root: serde_yaml::Value =
        serde_yaml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(root["cloud"]["agent_base_url"], "https://127.0.0.1:9210");
    assert_eq!(reopened.load_sync().unwrap().enabled, Some(true));
}

#[test]
fn concurrent_creation_and_retry_after_rejection_do_not_duplicate_enrollment() {
    let home = tempfile::tempdir().unwrap();
    let conf = home.path().join("conf");
    std::fs::create_dir_all(&conf).unwrap();
    let path = conf.join("talos.yaml");
    std::fs::write(&path, "{}").unwrap();
    let attempts = 8;
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(attempts));
    let joins: Vec<_> = (0..attempts)
        .map(|_| {
            let path = path.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                YamlConfigStore::new(path).begin_node_enrollment(None)
            })
        })
        .collect();
    let results: Vec<_> = joins.into_iter().map(|join| join.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert!(results
        .iter()
        .filter_map(|result| result.as_ref().err())
        .all(|error| error.is_code(ErrorCode::Conflict)));
    let reopened = YamlConfigStore::new(path.clone());
    assert!(reopened.node_enrollment_pending().unwrap());
    assert!(reopened
        .begin_node_enrollment(None)
        .unwrap_err()
        .is_code(ErrorCode::Conflict));
    reopened.cancel_node_creation().unwrap();
    assert!(!home.path().join(".worker-state.json").exists());
    assert!(!home.path().join(".worker-state.lock").exists());
    let restarted = YamlConfigStore::new(path);
    restarted.begin_node_enrollment(None).unwrap();
    assert!(restarted.node_enrollment_pending().unwrap());
    // Unreadable or damaged state cannot be treated as an empty registration.
    std::fs::write(home.path().join(".worker-state.json"), "{").unwrap();
    assert!(restarted.node_enrollment_pending().is_err());
    assert!(restarted.begin_node_enrollment(None).is_err());
}
