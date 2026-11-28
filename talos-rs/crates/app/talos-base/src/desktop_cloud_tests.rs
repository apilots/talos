use super::*;

use axum::Router;

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

#[tokio::test]
async fn remote_job_retries_after_server_account_bootstrap() {
    use axum::{
        http::{StatusCode, Uri},
        Json,
    };
    use serde_json::json;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    let step = Arc::new(AtomicUsize::new(0));
    let router = Router::new().fallback(move |method: Method, uri: Uri| {
        let step = step.fetch_add(1, Ordering::SeqCst);
        async move {
            match (step, method, uri.path()) {
                (0, Method::POST, "/v1/builtin-job/list") => (
                    StatusCode::UNAUTHORIZED,
                    Json(ApiResponse::error(talos_error::ApiError::new(ErrorCode::Unauthenticated))),
                ),
                (1, Method::GET, "/v1/user") => (StatusCode::OK, Json(ApiResponse::success(json!({
                    "user": {"user_id":"owner", "username":"owner", "role":"user", "type":"free",
                             "disabled":false, "created_at":0, "updated_at":0}
                })))),
                (2, Method::POST, "/v1/builtin-job/list") => (StatusCode::OK, Json(ApiResponse::success(json!({"data":[]})))),
                _ => panic!("unexpected bootstrap request {step}"),
            }
        }
    });
    let (cloud, server) = serve_cloud(router).await;
    let actor = Actor::regular("owner").with_access_token("token");
    let call = CloudCall {
        file_transfer: false,
        computer_id: None,
        method: Method::POST,
        path: "/v1/builtin-job/list",
        body: Some(json!({"computerId": "remote-worker"})),
    };
    assert_eq!(cloud.send(&actor, call).await.unwrap(), json!({"data":[]}));
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
