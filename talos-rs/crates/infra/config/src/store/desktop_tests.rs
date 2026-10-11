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
        bind_address: String::new(),
        tls_certificate_file: Default::default(),
        tls_private_key_file: Default::default(),
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
    assert!(!reopened.load_desktop().unwrap().enrollment_pending);
    assert!(reopened.begin_node_enrollment(None).is_err());
    reopened
        .save_desktop_connection(DesktopConnectionMode::Local)
        .unwrap();
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
    }
}

#[test]
fn failed_credential_save_keeps_uuid_for_recovery() {
    let home = tempfile::tempdir().unwrap();
    let path = home.path().join("talos.yaml");
    std::fs::write(&path, "{}").unwrap();
    let store = YamlConfigStore::new(path);
    let runner: AgentNodeRunnerConfig = serde_yaml::from_str(&format!(
        "node_id: stable-uuid\ncontrol_plane_endpoint: https://agent.talos.love\ncredential_file: {}\n", home.path().join("credential").display())).unwrap();
    std::fs::create_dir(&runner.credential_file).unwrap();
    assert!(store.save_node_identity(&runner, "TOKEN").is_err());
    assert!(store.load_desktop().unwrap().enrollment_pending);
    assert_eq!(
        store.load_agent_node().unwrap().runner.unwrap().node_id,
        "stable-uuid"
    );
}
