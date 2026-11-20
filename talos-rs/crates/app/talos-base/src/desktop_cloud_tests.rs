use super::*;

use axum::{extract::Request, http::StatusCode, Json, Router};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use talos_types::{BuiltinJobListRequest, ManagedWorkerResponse};

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
                        let identity = UserCurrentResponse {
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
                        };
                        (
                            StatusCode::OK,
                            Json(ApiResponse::success(value(&identity).unwrap())),
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
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let cloud = DesktopCloud::new(&CloudConfig {
        agent_base_url: endpoint,
        ..CloudConfig::default()
    })
    .unwrap();
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
