use crate::protocol::{initialize_service, map_service_error, McpClient, RmcpService};
use rmcp::model::{ClientRequest, ProtocolVersion, ServerResult};
use rmcp::service::ServiceError;
use serde_json::Value;
use talos_types::{McpCallToolRequest, McpCallToolResponse, McpContent};

#[test]
fn service_error_preserves_typed_code_without_exposing_remote_payload() {
    use std::error::Error;
    let error = map_service_error(ServiceError::McpError(rmcp::model::ErrorData::new(
        rmcp::model::ErrorCode(-32601),
        "SECRET_REMOTE_MESSAGE",
        Some(serde_json::json!({"token": "SECRET"})),
    )));
    assert_eq!(
        error.to_api_error().message,
        "The MCP server does not support this method."
    );
    assert!(
        matches!(error.source().unwrap().downcast_ref::<ServiceError>(), Some(ServiceError::McpError(remote)) if remote.code.0 == -32601 && remote.data.is_none())
    );
    assert!(!error.to_string().contains("SECRET"));
}

async fn connected_server(
    version: ProtocolVersion,
    responses: Vec<Option<ServerResult>>,
) -> (
    talos_error::TalosResult<(McpClient, RmcpService)>,
    tokio::task::JoinHandle<()>,
    std::sync::Arc<tokio::sync::Notify>,
) {
    use rmcp::model::{
        ClientJsonRpcMessage, DiscoverResult, ErrorData, InitializeResult, ServerCapabilities,
        ServerJsonRpcMessage,
    };
    use rmcp::transport::{IntoTransport, Transport};

    let received = std::sync::Arc::new(tokio::sync::Notify::new());
    let server_received = received.clone();
    let (server_io, client_io) = tokio::io::duplex(8192);
    let task = tokio::spawn(async move {
        let mut server = IntoTransport::<rmcp::RoleServer, _, _>::into_transport(server_io);
        let ClientJsonRpcMessage::Request(discover) = server.receive().await.unwrap() else {
            panic!("expected discovery");
        };
        assert!(matches!(
            discover.request,
            ClientRequest::DiscoverRequest(_)
        ));
        if version == ProtocolVersion::V_2026_07_28 {
            server
                .send(ServerJsonRpcMessage::response(
                    ServerResult::DiscoverResult(DiscoverResult::new(
                        vec![version],
                        ServerCapabilities::builder().enable_tools().build(),
                    )),
                    discover.id,
                ))
                .await
                .unwrap();
        } else {
            server
                .send(ServerJsonRpcMessage::error(
                    ErrorData::method_not_found::<rmcp::model::DiscoverRequestMethod>(),
                    Some(discover.id),
                ))
                .await
                .unwrap();
            let ClientJsonRpcMessage::Request(initialize) = server.receive().await.unwrap() else {
                panic!("expected legacy initialization");
            };
            let ClientRequest::InitializeRequest(request) = initialize.request else {
                panic!("expected legacy initialization");
            };
            assert_eq!(
                request.params.protocol_version,
                ProtocolVersion::V_2025_11_25
            );
            server
                .send(ServerJsonRpcMessage::response(
                    ServerResult::InitializeResult(
                        InitializeResult::new(ServerCapabilities::builder().enable_tools().build())
                            .with_protocol_version(version),
                    ),
                    initialize.id,
                ))
                .await
                .unwrap();
            assert!(matches!(
                server.receive().await,
                Some(ClientJsonRpcMessage::Notification(_))
            ));
        }
        for response in responses {
            let ClientJsonRpcMessage::Request(request) = server.receive().await.unwrap() else {
                panic!("expected tool call");
            };
            assert!(matches!(request.request, ClientRequest::CallToolRequest(_)));
            server_received.notify_one();
            if let Some(response) = response {
                server
                    .send(ServerJsonRpcMessage::response(response, request.id))
                    .await
                    .unwrap();
            } else {
                let Some(ClientJsonRpcMessage::Notification(notification)) = server.receive().await
                else {
                    panic!("expected cancellation notification");
                };
                let rmcp::model::ClientNotification::CancelledNotification(cancelled) =
                    notification.notification
                else {
                    panic!("expected cancellation notification");
                };
                assert_eq!(cancelled.params.request_id, Some(request.id));
            }
        }
        // Keep the transport alive until the client has consumed the last response.
        while server.receive().await.is_some() {}
    });
    let client = initialize_service(client_io, &std::sync::Arc::default()).await;
    (
        client.map(|session| {
            let (service, _) = session.into_parts();
            (
                McpClient {
                    peer: service.peer().clone(),
                },
                service,
            )
        }),
        task,
        received,
    )
}

#[tokio::test]
async fn modern_and_legacy_servers_return_complete_tool_results() {
    for version in [ProtocolVersion::V_2026_07_28, ProtocolVersion::V_2025_06_18] {
        let mut result = rmcp::model::CallToolResult::error(vec![rmcp::model::ContentBlock::text(
            "tool rejected",
        )]);
        result.structured_content = Some(serde_json::json!(42));
        let (server, task, _) =
            connected_server(version, vec![Some(ServerResult::CallToolResult(result))]).await;
        let (server, service) = server.unwrap();
        let response = server
            .call_tool(
                McpCallToolRequest {
                    server_name: "fixture".into(),
                    tool_name: "example".into(),
                    arguments: Value::Null,
                },
                std::future::pending(),
            )
            .await
            .unwrap();
        assert_eq!(
            response,
            McpCallToolResponse {
                content: vec![McpContent::Text {
                    text: "tool rejected".into()
                }],
                is_error: true,
                structured_content: Some(serde_json::json!(42)),
            }
        );
        service.cancel().await.unwrap();
        task.await.unwrap();
    }
}

#[tokio::test]
async fn unsupported_input_and_audio_do_not_become_empty_success() {
    let responses = vec![
        ServerResult::InputRequiredResult(rmcp::model::InputRequiredResult::from_request_state(
            "opaque",
        )),
        ServerResult::CallToolResult(rmcp::model::CallToolResult::success(vec![
            rmcp::model::ContentBlock::audio("AA==", "audio/wav"),
        ])),
    ];
    let (server, task, _) = connected_server(
        ProtocolVersion::V_2026_07_28,
        responses.into_iter().map(Some).collect(),
    )
    .await;
    let (server, service) = server.unwrap();
    for expected in [
        talos_error::ErrorCode::ToolExecutionFailed,
        talos_error::ErrorCode::UnsupportedMediaType,
    ] {
        let error = server
            .call_tool(
                McpCallToolRequest {
                    server_name: "fixture".into(),
                    tool_name: "example".into(),
                    arguments: Value::Null,
                },
                std::future::pending(),
            )
            .await
            .unwrap_err();
        assert_eq!(error.to_api_error().code, expected);
    }
    service.cancel().await.unwrap();
    task.await.unwrap();
}

#[tokio::test]
async fn unknown_legacy_version_is_rejected_before_calls() {
    let version = serde_json::from_value(serde_json::json!("2099-01-01")).unwrap();
    let (server, task, _) = connected_server(version, vec![]).await;
    let error = server
        .err()
        .expect("unsupported protocol must fail initialization");
    assert_eq!(
        error.to_api_error().message,
        "The MCP server selected an unsupported protocol version."
    );
    task.await.unwrap();
}

#[tokio::test]
async fn cancelled_call_notifies_server_and_keeps_connection_usable() {
    let (server, task, received) = connected_server(
        ProtocolVersion::V_2026_07_28,
        vec![
            None,
            Some(ServerResult::CallToolResult(
                rmcp::model::CallToolResult::success(vec![rmcp::model::ContentBlock::text(
                    "still usable",
                )]),
            )),
        ],
    )
    .await;
    let (server, service) = server.unwrap();
    let request = McpCallToolRequest {
        server_name: "fixture".into(),
        tool_name: "example".into(),
        arguments: Value::Null,
    };
    let error = server
        .call_tool(request.clone(), std::future::ready(()))
        .await
        .unwrap_err();
    assert_eq!(error.error_code(), talos_error::ErrorCode::Cancelled);
    let error = server
        .call_tool(request.clone(), received.notified())
        .await
        .unwrap_err();
    assert_eq!(error.error_code(), talos_error::ErrorCode::Cancelled);
    let result = server
        .call_tool(request, std::future::pending())
        .await
        .unwrap();
    assert_eq!(
        result.content,
        vec![McpContent::Text {
            text: "still usable".into()
        }]
    );
    service.cancel().await.unwrap();
    task.await.unwrap();
}
