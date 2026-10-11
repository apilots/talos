use super::*;

#[test]
fn mcp_install_credentials_preserve_absent_and_present_empty_maps() {
    for headers in [None, Some(std::collections::BTreeMap::new())] {
        let request = d::PluginInstallRequest {
            node_id: None,
            target: d::PluginInstallTarget::ManualMcpServer {
                server: Box::new(d::McpServerDefinition {
                    credentials: None,
                    name: "personal-mcp".into(),
                    display_name: Some("Personal MCP".into()),
                    description: None,
                    transport: d::McpTransportDefinition::StreamableHttp {
                        url: "https://mcp.example.com".into(),
                        headers: Default::default(),
                    },
                }),
                credentials: Some(d::McpCredentialValues {
                    authentication: d::McpAuthentication::Bearer,
                    token: Some("private-write-only-fixture".into()),
                    env: None,
                    headers,
                }),
            },
            enable: Some(false),
            conflict_policy: d::PluginInstallConflictPolicy::Reject,
        };
        assert_eq!(
            decode_install_plugin_params(Some(
                encode_install_plugin_params(request.clone()).unwrap()
            ))
            .unwrap(),
            request
        );
    }
}

#[test]
fn mcp_status_keeps_the_original_public_failure_and_page_cursor() {
    let response = d::McpServerListResponse {
        data: vec![d::McpServerInfo {
            name: "personal-mcp".into(),
            display_name: None,
            source: d::McpServerSource::Plugin {
                plugin_id: d::PluginId("plugin-1".into()),
                install_id: d::PluginInstallId("install-1".into()),
            },
            transport: d::McpTransportKind::StreamableHttp,
            status: d::McpServerStatus::Failed {
                error: talos_error::TalosError::new(
                    talos_error::ErrorCode::Unauthenticated,
                    "MCP authentication was rejected",
                )
                .to_api_error(),
            },
            tool_count: 0,
        }],
        next_cursor: Some("page-2".into()),
    };
    let expected = serde_json::to_value(&response).unwrap();
    let response =
        decode_list_mcp_servers_result(encode_list_mcp_servers_result(response).unwrap()).unwrap();
    assert_eq!(serde_json::to_value(response).unwrap(), expected);
}
