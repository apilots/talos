use std::fs;

use super::*;

#[tokio::test]
async fn standalone_direct_worker_cannot_borrow_an_unserved_control_plane_tls_identity() {
    let mut config = Config::default();
    config.agent_node.control_plane = Some(talos_config::AgentNodeControlConfig {
        bind_address: "127.0.0.1:9443".into(),
        tls_certificate_file: "server.crt".into(),
        tls_private_key_file: "server.key".into(),
        trusted_node_ca_file: None,
    });
    assert_eq!(
        run_with_config(config).await.unwrap_err().error_code(),
        ErrorCode::InvalidRequest
    );
}

#[test]
fn registration_credential_must_be_a_private_file_outside_workspace() {
    let root = tempfile::tempdir().unwrap();
    let conf = root.path().join("conf");
    let workspace = root.path().join("workspace-talos");
    fs::create_dir_all(&conf).unwrap();
    fs::create_dir_all(&workspace).unwrap();
    let credential = conf.join("node.credential");
    fs::write(&credential, "registration-secret\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&credential, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(read_credential(&credential, &conf).is_err());
        fs::set_permissions(&credential, fs::Permissions::from_mode(0o600)).unwrap();
    }
    assert_eq!(
        read_credential(&credential, &conf).unwrap(),
        "registration-secret"
    );

    let misplaced = workspace.join("node.credential");
    fs::write(&misplaced, "registration-secret").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&misplaced, fs::Permissions::from_mode(0o600)).unwrap();
    }
    assert!(read_credential(&misplaced, &conf).is_err());
}
