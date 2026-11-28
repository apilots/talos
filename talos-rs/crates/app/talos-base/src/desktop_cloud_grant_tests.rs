use super::*;
use axum::{
    extract::{
        ws::{Message, WebSocketUpgrade},
        Query,
    },
    routing::{get, post},
    Json, Router,
};
use serde_json::json;
use std::{collections::HashMap, sync::Arc};
use talos_app_protocol::requests::{ClientMethod, ClientRequest, ClientRequestParams};
use talos_app_server::{AppServerApplication, DesktopHostBridge, DesktopHostEvent};
use talos_service::Actor;
use talos_types::{BrowserInputMode, DesktopOpenRequest, DesktopStreamOpen, DesktopViewGrant};

#[tokio::test]
async fn remote_desktop_grant_preserves_its_binding_and_opens_at_its_issuing_server() {
    let grant = DesktopViewGrant {
        ticket: "server-issued-ticket".into(),
        stream_id: uuid::Uuid::new_v4().to_string(),
        viewer_id: uuid::Uuid::new_v4().to_string(),
        generation: uuid::Uuid::new_v4().to_string(),
        input_mode: BrowserInputMode::ReadOnly,
        controller_id: None,
        input_epoch: None,
        expires_at: chrono::Utc::now().timestamp() + 60,
    };
    let issued = grant.clone();
    let stream_id = grant.stream_id.clone();
    let router = Router::new()
        .route(
            "/v1/desktop/open",
            post(move || {
                let grant = issued.clone();
                async move { Json(talos_types::ApiResponse::success(grant)) }
            }),
        )
        .route(
            "/v1/desktop/stream",
            get(
                move |Query(query): Query<HashMap<String, String>>, ws: WebSocketUpgrade| {
                    assert_eq!(query.get("streamId"), Some(&stream_id));
                    async move {
                        ws.on_upgrade(|mut socket| async move {
                let Some(Ok(Message::Text(credentials))) = socket.recv().await else {
                    panic!("Server must receive stream credentials");
                };
                assert_eq!(serde_json::from_str::<serde_json::Value>(&credentials).unwrap(),
                    json!({"ticket":"server-issued-ticket", "accessToken":"owner-token"}));
                socket.send(Message::Binary(b"RFB 003.008\n".to_vec())).await.unwrap();
                let _ = socket.recv().await;
            })
                    }
                },
            ),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let mut config = Config::default();
    let workspace = isolate_runtime_workspace(&mut config, "remote-desktop-grant");
    config.gateway.auth = false;
    config.storage.connection_string = ":memory:".into();
    config.llm.anthropic.models = vec![provider_model(&config.llm.model)];
    fs::create_dir_all(config.source_path().parent().unwrap()).unwrap();
    config.save(config.source_path().to_str().unwrap()).unwrap();
    let mut runtime = build_application_runtime_with_migration_dir(config, &test_migrations_dir())
        .await
        .unwrap();
    // Registration keeps its TLS endpoint; this test only serves the product API.
    runtime.config.cloud.agent_base_url = endpoint;
    let bridge = DesktopHostBridge::new();
    bridge.activate();
    let mut frames = bridge.take_receiver().unwrap();
    let app = crate::desktop_application::DesktopApplication::new(
        &runtime,
        "Viewer".into(),
        Arc::new(talos_app_server::TalosApplicationAdapter::new(
            runtime.service.clone(),
            20,
        )),
        bridge.clone(),
    );
    let actor = Actor::regular("owner").with_access_token("owner-token");
    let response = app
        .dispatch(
            &actor,
            ClientRequest {
                computer_id: None,
                method: ClientMethod::DesktopOpen,
                params: ClientRequestParams::DesktopOpen(DesktopOpenRequest {
                    node_id: "remote-computer".into(),
                    target_id: "managed".into(),
                    viewer_id: grant.viewer_id.clone(),
                    generation: grant.generation.clone(),
                    input_mode: grant.input_mode,
                    controller_id: grant.controller_id.clone(),
                    input_epoch: grant.input_epoch,
                }),
            },
        )
        .await
        .unwrap();
    let received: DesktopViewGrant = serde_json::from_value(response.body).unwrap();
    let mut expected = grant;
    expected.ticket = format!("{}{}", crate::desktop_cloud::SERVER_TICKET, expected.ticket);
    assert_eq!(received, expected);
    app.dispatch(
        &actor,
        ClientRequest {
            computer_id: None,
            method: ClientMethod::DesktopStreamOpen,
            params: ClientRequestParams::DesktopStreamOpen(DesktopStreamOpen {
                stream_id: received.stream_id.clone(),
                ticket: received.ticket,
            }),
        },
    )
    .await
    .unwrap();
    let Some(DesktopHostEvent::Media { id, frame }) =
        tokio::time::timeout(Duration::from_secs(3), frames.recv())
            .await
            .unwrap()
    else {
        panic!("Remote Server must deliver desktop media");
    };
    assert_eq!(frame.stream_id, expected.stream_id);
    assert_eq!(frame.data_base64.as_deref(), Some("UkZCIDAwMy4wMDgK"));
    bridge.complete(&id, Ok(json!({"kind":"done"})));
    app.close_connection();
    runtime.shutdown().await.unwrap();
    runtime.shutdown_host().await.unwrap();
    server.abort();
    fs::remove_dir_all(workspace).unwrap();
}
