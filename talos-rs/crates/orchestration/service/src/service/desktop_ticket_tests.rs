use super::super::tests::{
    configured_test_config, test_application, TestAgentExecution, TestAgentExecutionMode,
};
use super::*;
use std::sync::Arc;

async fn service() -> TalosService {
    test_application(
        Arc::new(TestAgentExecution {
            mode: TestAgentExecutionMode::Accepted,
        }),
        configured_test_config(),
    )
    .await
    .0
}

fn seed_ticket(service: &TalosService, actor: Actor) -> String {
    let ticket = uuid::Uuid::new_v4().to_string();
    service.desktop_routes.lock().unwrap().insert(
        Sha256::digest(ticket.as_bytes()).into(),
        ViewerRoute {
            actor,
            node_id: "embedded".into(),
            binding: DesktopStreamBinding {
                stream_id: uuid::Uuid::new_v4().to_string(),
                target_id: "managed".into(),
                viewer_id: uuid::Uuid::new_v4().to_string(),
                generation: uuid::Uuid::new_v4().to_string(),
                input_mode: BrowserInputMode::ReadOnly,
                controller_id: None,
                input_epoch: None,
            },
            ingress_ticket: uuid::Uuid::new_v4().to_string(),
            expires: std::time::Instant::now() + std::time::Duration::from_secs(30),
        },
    );
    ticket
}

#[tokio::test]
async fn anonymous_desktop_admission_only_routes_to_the_embedded_worker() {
    let service = service().await;
    for (node_id, expected) in [
        ("embedded".to_string(), ErrorCode::ServiceUnavailable),
        (
            uuid::Uuid::new_v4().to_string(),
            ErrorCode::PermissionDenied,
        ),
    ] {
        let error = service
            .open_desktop(
                &Actor::auth_disabled(),
                DesktopOpenRequest {
                    node_id,
                    target_id: "managed".into(),
                    viewer_id: uuid::Uuid::new_v4().to_string(),
                    generation: uuid::Uuid::new_v4().to_string(),
                    input_mode: BrowserInputMode::ReadOnly,
                    controller_id: None,
                    input_epoch: None,
                },
            )
            .await
            .unwrap_err();
        // Local admission reaches the deliberately unbundled fixture's environment.
        assert!(error.is_code(expected), "{error:?}");
    }
}

#[tokio::test]
async fn desktop_tickets_bind_the_account_and_auth_mode_and_are_consumed_once() {
    let service = service().await;
    let anonymous = Actor::auth_disabled();
    let owner = Actor::regular("owner");
    for (issued_to, presented_by, expected) in [
        (
            anonymous.clone(),
            Actor::regular("anonymous"),
            ErrorCode::PermissionDenied,
        ),
        (
            owner.clone(),
            Actor::regular("other"),
            ErrorCode::PermissionDenied,
        ),
        (anonymous.clone(), anonymous, ErrorCode::ServiceUnavailable),
        (owner.clone(), owner, ErrorCode::ServiceUnavailable),
    ] {
        let ticket = seed_ticket(&service, issued_to.clone());
        let error = service
            .consume_desktop_ticket(&presented_by, ticket.clone(), None)
            .await
            .err()
            .expect("The unbundled fixture cannot produce a desktop transport");
        assert!(error.is_code(expected), "{error:?}");
        let replay = service
            .consume_desktop_ticket(&issued_to, ticket, None)
            .await
            .err()
            .expect("Consumed grants cannot be replayed");
        assert!(replay.is_code(ErrorCode::PermissionDenied));
        assert!(service.desktop_routes.lock().unwrap().is_empty());
    }
}
