use super::*;

#[test]
fn node_network_configuration_requires_authenticated_tls() {
    let control = AgentNodeControlConfig {
        bind_address: "127.0.0.1:18800".to_string(),
        tls_certificate_file: "server.crt".into(),
        tls_private_key_file: "server.key".into(),
        trusted_node_ca_file: Some("node-ca.crt".into()),
    };
    let mut config = AgentNodeConfig {
        control_plane: Some(control),
        runner: None,
        worker_rpc: WorkerRpcConfig::default(),
    };
    let mut gateway = GatewayConfig::default();
    assert!(config.validate(&gateway).is_err());

    gateway.auth = true;
    config.control_plane.as_mut().unwrap().trusted_node_ca_file = None;
    assert!(config.validate(&gateway).is_ok());
    config.control_plane.as_mut().unwrap().trusted_node_ca_file = Some(PathBuf::new());
    assert!(config.validate(&gateway).is_err());
    config.control_plane.as_mut().unwrap().trusted_node_ca_file = Some("node-ca.crt".into());
    let runner: AgentNodeRunnerConfig = serde_yaml::from_str(
        "transport: direct\nnode_id: laptop-1\ncontrol_plane_endpoint: http://localhost:18800\ncredential_file: /tmp/node.credential\ntrusted_ca_file: ca.crt\n",
    )
    .unwrap();
    config.runner = Some(runner);
    config.worker_rpc.tls = Some(WorkerRpcTlsConfig {
        certificate_file: "node.crt".into(),
        private_key_file: "node.key".into(),
    });
    assert!(config.validate(&gateway).is_err());

    config.runner.as_mut().unwrap().control_plane_endpoint = "https://localhost:18800".to_string();
    assert!(config.validate(&gateway).is_ok());
    config.runner.as_mut().unwrap().trusted_ca_file = None;
    assert!(config.validate(&gateway).is_ok());

    // A central Server serves direct RPC on its existing authenticated TLS ingress.
    config.worker_rpc.tls = None;
    assert!(config.validate(&gateway).is_ok());
    let control = config.control_plane.take();
    assert!(config.validate(&gateway).is_err());
    config.control_plane = control;

    let registration_only: AgentNodeRunnerConfig = serde_yaml::from_str(
        "node_id: laptop-1\ncontrol_plane_endpoint: https://localhost:18800\ncredential_file: /tmp/node.credential\ntrusted_ca_file: ca.crt\n",
    )
    .unwrap();
    config.runner = Some(registration_only);
    assert!(config.validate(&gateway).is_ok());
    config.worker_rpc.bind_address = "127.0.0.1:18801".to_string();
    assert!(config.validate(&gateway).is_ok());
    config.worker_rpc.tls = None;
    config.worker_rpc.bind_address = "0.0.0.0:18801".to_string();
    assert!(config.validate(&gateway).is_err());
    config.worker_rpc.bind_address = "[::1]:0".to_string();
    assert!(config.validate(&gateway).is_ok());
}

#[test]
fn personal_outbound_and_cloud_grpc_are_explicit_without_fallback() {
    let mut runner: AgentNodeRunnerConfig = serde_yaml::from_str(
        "node_id: laptop-1\ncontrol_plane_endpoint: https://localhost:18800\ncredential_file: /tmp/node.credential\n"
    ).unwrap();
    assert_eq!(runner.transport(), AgentNodeTransport::Outbound);
    assert_eq!(
        runner.transport_for_packaging(None),
        AgentNodeTransport::Outbound
    );
    assert_eq!(
        runner.transport_for_packaging(Some(std::ffi::OsStr::new("official-container"))),
        AgentNodeTransport::OutboundGrpc
    );
    assert_eq!(
        runner.transport_for_packaging(Some(std::ffi::OsStr::new("native"))),
        AgentNodeTransport::Outbound
    );
    runner.transport = Some(AgentNodeTransport::OutboundGrpc);
    let encoded = serde_yaml::to_string(&runner).unwrap();
    assert!(encoded.contains("outboundGrpc"));
    assert_eq!(
        serde_yaml::from_str::<AgentNodeRunnerConfig>(&encoded)
            .unwrap()
            .transport(),
        AgentNodeTransport::OutboundGrpc
    );
}

#[test]
fn official_managed_seed_stays_loadable_by_older_worker_images() {
    #[derive(Debug, serde::Deserialize, PartialEq, Eq)]
    #[serde(rename_all = "camelCase")]
    enum LegacyTransport {
        Outbound,
        Direct,
    }

    #[derive(Debug, serde::Deserialize)]
    struct LegacyRunner {
        transport: Option<LegacyTransport>,
    }

    #[derive(Debug, serde::Deserialize)]
    struct LegacySeed {
        agent_node: LegacyAgentNode,
    }

    #[derive(Debug, serde::Deserialize)]
    struct LegacyAgentNode {
        runner: LegacyRunner,
    }

    let seed = crate::managed_worker_seed_config(AgentNodeRunnerConfig {
        transport: Some(AgentNodeTransport::Outbound),
        node_id: "worker-1".into(),
        control_plane_endpoint: "https://agent.example".into(),
        credential_file: "/home/talos/.talos/conf/node.credential".into(),
        trusted_ca_file: Some("/home/talos/.talos/conf/ca.crt".into()),
    })
    .unwrap();
    let encoded = String::from_utf8(seed).unwrap();
    assert!(encoded.contains("transport: outbound"));
    assert!(!encoded.contains("outboundGrpc"));
    let legacy = serde_yaml::from_str::<LegacySeed>(&encoded).unwrap();
    assert_eq!(
        legacy.agent_node.runner.transport,
        Some(LegacyTransport::Outbound)
    );

    let mut runner = AgentNodeRunnerConfig {
        transport: Some(AgentNodeTransport::Outbound),
        node_id: "worker-1".into(),
        control_plane_endpoint: "https://agent.example".into(),
        credential_file: "/home/talos/.talos/conf/node.credential".into(),
        trusted_ca_file: None,
    };
    assert_eq!(
        runner.transport_for_packaging(Some(std::ffi::OsStr::new("official-container"))),
        AgentNodeTransport::OutboundGrpc
    );
    runner.transport = Some(AgentNodeTransport::OutboundGrpc);
    let incompatible = serde_yaml::to_string(&runner).unwrap();
    assert!(serde_yaml::from_str::<LegacyRunner>(&incompatible).is_err());
}
