use super::*;

use axum::{extract::Request, http::StatusCode, Json, Router};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use talos_types::{BuiltinJobListRequest, ManagedWorkerResponse};

async fn serve_cloud(router: Router) -> (DesktopCloud, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let cloud = DesktopCloud::new(&CloudConfig {
        agent_base_url: endpoint,
        ..CloudConfig::default()
    })
    .unwrap();
    (cloud, server)
}

fn test_identity() -> UserCurrentResponse {
    UserCurrentResponse {
        user: talos_auth::UserDto {
            user_id: "test-user".into(),
            username: "example".into(),
            email: None,
            role: talos_auth::UserRole::User,
            r#type: talos_auth::UserType::Max,
            disabled: false,
            created_at: 0,
            updated_at: 0,
        },
    }
}

#[tokio::test]
async fn cloud_calls_use_the_server_origin_and_preserve_authorization_errors() {
    let admitted = Arc::new(AtomicBool::new(false));
    let router = Router::new().fallback({
        let admitted = admitted.clone();
        move |request: Request| {
            let admitted = admitted.clone();
            async move {
                assert_eq!(request.headers()["authorization"], "Bearer test-account");
                match (request.method().clone(), request.uri().path()) {
                    (Method::POST, "/v1/builtin-job/list") => {
                        let body = axum::body::to_bytes(request.into_body(), 1024)
                            .await
                            .unwrap();
                        assert_eq!(
                            serde_json::from_slice::<Value>(&body).unwrap(),
                            json!({"computerId": "cloud-computer"})
                        );
                        (
                            StatusCode::FORBIDDEN,
                            Json(ApiResponse::error(talos_error::ApiError::new(
                                ErrorCode::PermissionDenied,
                            ))),
                        )
                    }
                    (Method::GET, "/v1/user") => {
                        assert!(!admitted.swap(true, Ordering::SeqCst));
                        (
                            StatusCode::OK,
                            Json(ApiResponse::success(value(&test_identity()).unwrap())),
                        )
                    }
                    (Method::GET, TALO_PATH) if !admitted.load(Ordering::SeqCst) => (
                        StatusCode::UNAUTHORIZED,
                        Json(ApiResponse::error(talos_error::ApiError::new(
                            ErrorCode::Unauthenticated,
                        ))),
                    ),
                    (Method::GET, TALO_PATH) | (Method::POST, TALO_PATH) => (
                        StatusCode::OK,
                        Json(ApiResponse::success(
                            json!({"enabled": true, "instance": null}),
                        )),
                    ),
                    (Method::POST, action)
                        if action
                            == format!(
                                "{TALO_PATH}/9a2dc23a-8d65-47c1-8ed0-6ebae4cb64ba/start"
                            ) =>
                    {
                        let body = axum::body::to_bytes(request.into_body(), 1024)
                            .await
                            .unwrap();
                        assert_eq!(
                            serde_json::from_slice::<Value>(&body).unwrap(),
                            json!({"expectedGeneration": 4})
                        );
                        (
                            StatusCode::OK,
                            Json(ApiResponse::success(
                                json!({"enabled": true, "instance": null}),
                            )),
                        )
                    }
                    _ => panic!("unexpected cloud call"),
                }
            }
        }
    });
    let (cloud, server) = serve_cloud(router).await;
    let actor = Actor::regular("test-user").with_access_token("test-account");
    let call = CloudCall::from_request(&ClientRequestParams::BuiltinJobList(
        BuiltinJobListRequest {
            computer_id: "cloud-computer".into(),
        },
    ))
    .unwrap()
    .unwrap();
    assert!(cloud
        .send(&actor, call)
        .await
        .unwrap_err()
        .is_code(ErrorCode::PermissionDenied));
    for params in [
        ClientRequestParams::ComputerReadManagedWorker(None),
        ClientRequestParams::ComputerCreateManagedWorker(None),
        ClientRequestParams::ComputerStartManagedWorker(ManagedWorkerActionRequest {
            instance_id: "9a2dc23a-8d65-47c1-8ed0-6ebae4cb64ba".into(),
            expected_generation: 4,
        }),
    ] {
        let call = CloudCall::from_request(&params).unwrap().unwrap();
        let data = cloud.send(&actor, call).await.unwrap();
        let status: ManagedWorkerResponse = serde_json::from_value(data).unwrap();
        assert!(status.enabled && status.instance.is_none());
    }
    server.abort();
    assert!(admitted.load(Ordering::SeqCst));
}

#[tokio::test]
async fn personal_viewer_requires_media_before_opening_and_delivers_the_first_frame() {
    use axum::extract::ws::{Message, WebSocketUpgrade};
    const BANNER: &[u8] = b"RFB 003.008\n";
    let router = Router::new().route(
        "/v1/desktop/stream",
        axum::routing::get(|ws: WebSocketUpgrade| async {
            ws.on_upgrade(|mut socket| async move {
                let Some(Ok(Message::Text(ticket))) = socket.recv().await else {
                    panic!("expected a ticket");
                };
                let credentials = serde_json::from_str::<Value>(&ticket).unwrap();
                if credentials["ticket"] == "ready" {
                    socket.send(Message::Binary(BANNER.to_vec())).await.unwrap();
                }
                let _ = socket.recv().await;
            })
        }),
    );
    let (cloud, server) = serve_cloud(router).await;
    let actor = Actor::regular("test-user").with_access_token("test-account");
    let stream_id = uuid::Uuid::new_v4().to_string();
    let stalled = tokio::time::timeout(
        Duration::from_secs(10),
        cloud.connect_viewer(&actor, "stalled", &stream_id),
    )
    .await
    .expect("silent viewer admission must finish");
    assert_eq!(
        stalled
            .err()
            .expect("a silent viewer cannot open")
            .error_code(),
        ErrorCode::Timeout
    );
    let mut transport = cloud
        .connect_viewer(&actor, "ready", &stream_id)
        .await
        .unwrap();
    let first = tokio::time::timeout(Duration::from_secs(1), transport.incoming.recv())
        .await
        .expect("first frame should be delivered")
        .unwrap()
        .unwrap();
    assert_eq!(first, BANNER);
    server.abort();
}
