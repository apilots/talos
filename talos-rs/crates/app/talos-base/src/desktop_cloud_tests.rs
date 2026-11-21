use super::*;

use axum::{extract::Request, http::StatusCode, Json, Router};
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
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
        .send::<Value>(&actor, call)
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
        let data = cloud.dispatch(&actor, call, &params).await.unwrap();
        let status: ManagedWorkerResponse = serde_json::from_value(data).unwrap();
        assert!(status.enabled && status.instance.is_none());
    }
    server.abort();
    assert!(admitted.load(Ordering::SeqCst));
}

#[tokio::test]
async fn personal_timeline_reconnects_with_an_uncached_server_credential() {
    use axum::response::IntoResponse;
    use talos_service::{TimelineSubscriptionCloseReason, TimelineSubscriptionMessage};

    let requests = Arc::new(AtomicUsize::new(0));
    let router = Router::new().fallback({
        let requests = requests.clone();
        move |request: Request| {
            let step = requests.fetch_add(1, Ordering::SeqCst);
            async move {
                assert_eq!(request.headers()["authorization"], "Bearer test-account");
                match (step, request.uri().path()) {
                    (1, "/v1/user") => Json(ApiResponse::success(test_identity())).into_response(),
                    (0 | 2, "/v1/sessions/personal/events") => {
                        assert_eq!(
                            request.uri().query().unwrap(),
                            "after_event_id=cursor&turn_id=turn&limit=3&close_when_idle=false&close_on_terminal=true"
                        );
                        if step == 0 {
                            return (
                                StatusCode::UNAUTHORIZED,
                                Json(ApiResponse::<Value>::error(talos_error::ApiError::new(
                                    ErrorCode::Unauthenticated,
                                ))),
                            )
                                .into_response();
                        }
                        (
                            [("content-type", "text/event-stream")],
                            "event: subscription/terminated\ndata: {\"reason\":\"completed\",\"message\":\"done\"}\n\n",
                        )
                            .into_response()
                    }
                    _ => panic!("unexpected reconnect request"),
                }
            }
        }
    });
    let (cloud, server) = serve_cloud(router).await;
    let actor = Actor::regular("test-user").with_access_token("test-account");
    let mut subscription = cloud
        .subscribe(
            &actor,
            talos_types::SubscribeTimelineRequest {
                session_id: "personal".into(),
                turn_id: Some("turn".into()),
                since_event_id: Some("cursor".into()),
                replay_limit: Some(3),
                close_when_idle: Some(false),
                close_on_terminal: Some(true),
            },
        )
        .await
        .unwrap();
    let Some(TimelineSubscriptionMessage::Closed(event)) = subscription.events.recv().await else {
        panic!("expected a terminal event on the recovered stream");
    };
    assert_eq!(event.reason, TimelineSubscriptionCloseReason::Completed);
    assert_eq!(requests.load(Ordering::SeqCst), 3);
    server.abort();
}

#[tokio::test]
async fn personal_timeline_forwards_the_requested_subscription_lifetime() {
    #[derive(serde::Deserialize)]
    struct CloseOptions {
        close_when_idle: bool,
        close_on_terminal: bool,
    }

    let (send, mut received) = tokio::sync::mpsc::unbounded_channel();
    let router = Router::new().route(
        "/v1/sessions/personal/events",
        axum::routing::get(
            move |axum::extract::Query(options): axum::extract::Query<CloseOptions>| async move {
                send.send(options).unwrap();
                StatusCode::OK
            },
        ),
    );
    let (cloud, server) = serve_cloud(router).await;
    let actor = Actor::regular("test-user").with_access_token("test-account");
    for (close_when_idle, close_on_terminal) in [
        (None, None),
        (Some(true), Some(false)),
        (Some(false), Some(true)),
    ] {
        cloud
            .subscribe(
                &actor,
                talos_types::SubscribeTimelineRequest {
                    session_id: "personal".into(),
                    turn_id: Some("turn".into()),
                    since_event_id: None,
                    replay_limit: None,
                    close_when_idle,
                    close_on_terminal,
                },
            )
            .await
            .unwrap();
        let options = received.recv().await.unwrap();
        assert_eq!(
            (options.close_when_idle, options.close_on_terminal),
            (
                close_when_idle.unwrap_or(false),
                close_on_terminal.unwrap_or(false)
            )
        );
    }
    server.abort();
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

#[test]
fn closing_a_cloud_preview_releases_the_original_server_grant() {
    let request = ClientRequestParams::FilePreviewClose(talos_types::FilePreviewCloseRequest {
        preview_id: format!("{SERVER_PREVIEW}grant"),
    });
    let call = CloudCall::from_request(&request).unwrap().unwrap();
    assert_eq!(call.path, "/v1/file-previews/close");
    assert!(
        matches!(call.body, Some(CloudBody::Json(body)) if body == serde_json::json!({"previewId":"grant"}))
    );
}
