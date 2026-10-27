use super::*;
use std::sync::atomic::{AtomicBool, Ordering};
use talos_auth::{RequestIdentity, UserRole};
use talos_types::{BrowserInputMode, WorkerDesktopOpen};
use tokio_util::sync::CancellationToken;

struct LocalAuth(AtomicBool);

#[async_trait::async_trait]
impl AuthService for LocalAuth {
    async fn logout(&self, _: &RequestIdentity) -> TalosResult<()> {
        self.0.store(false, Ordering::Release);
        Ok(())
    }

    fn local_identity(&self, token: &str) -> TalosResult<RequestIdentity> {
        if token != "verified-local-session" || !self.0.load(Ordering::Acquire) {
            return Err(TalosError::new(
                ErrorCode::Unauthenticated,
                "Session was revoked.",
            ));
        }
        Ok(RequestIdentity {
            user_id: "alice".into(),
            username: None,
            role: UserRole::User,
            auth_disabled: false,
            access_token: Some(token.into()),
        })
    }

    async fn verify_access_token(&self, _: &str) -> TalosResult<RequestIdentity> {
        panic!("Desktop data access must not verify through Cloud")
    }
}

async fn ingress(auth: Arc<LocalAuth>) -> ApplicationDesktopIngress {
    ApplicationDesktopIngress {
        local: DesktopRelay::new(CancellationToken::new()),
        remote: AgentNodeClient::connect(
            "https://127.0.0.1:1",
            agent_client::AgentTlsTrust::SystemRoots,
        )
        .await
        .unwrap(),
        auth,
    }
}

async fn pair(
    ingress: &ApplicationDesktopIngress,
    actor: &Actor,
) -> (DesktopTransport, DesktopTransport) {
    let binding = DesktopStreamBinding {
        stream_id: uuid::Uuid::new_v4().to_string(),
        target_id: "managed".into(),
        viewer_id: uuid::Uuid::new_v4().to_string(),
        generation: uuid::Uuid::new_v4().to_string(),
        input_mode: BrowserInputMode::ReadOnly,
        controller_id: None,
        input_epoch: None,
    };
    let reservation = ingress
        .reserve(actor, "embedded".into(), binding.clone())
        .await
        .unwrap();
    let worker = ingress
        .local
        .attach_local_worker(WorkerDesktopOpen {
            binding: binding.clone(),
            nonce: reservation.nonce,
        })
        .await
        .unwrap();
    let viewer = ingress
        .view(
            actor,
            "embedded".into(),
            binding.stream_id,
            reservation.ticket,
        )
        .await
        .unwrap();
    (worker, viewer)
}

#[tokio::test]
async fn anonymous_embedded_media_works_without_cloud_and_cannot_route_to_remote() {
    let ingress = ingress(Arc::new(LocalAuth(AtomicBool::new(false)))).await;
    let actor = Actor::auth_disabled();
    let (worker, mut viewer) = pair(&ingress, &actor).await;
    worker
        .outgoing
        .send(b"local desktop bytes".to_vec())
        .await
        .unwrap();
    assert_eq!(
        viewer.incoming.recv().await.unwrap().unwrap(),
        b"local desktop bytes"
    );
    assert!(ingress
        .view(
            &actor,
            "registered-node".into(),
            uuid::Uuid::new_v4().to_string(),
            uuid::Uuid::new_v4().to_string()
        )
        .await
        .err()
        .unwrap()
        .is_code(ErrorCode::PermissionDenied));
}

#[tokio::test]
async fn authenticated_local_media_uses_cached_identity_and_closes_on_logout() {
    let auth = Arc::new(LocalAuth(AtomicBool::new(true)));
    let ingress = ingress(auth.clone()).await;
    let actor = Actor::regular("alice").with_access_token("verified-local-session");
    let (mut worker, viewer) = pair(&ingress, &actor).await;
    viewer.outgoing.send(b"input bytes".to_vec()).await.unwrap();
    assert_eq!(
        worker.incoming.recv().await.unwrap().unwrap(),
        b"input bytes"
    );
    auth.logout(&auth.local_identity("verified-local-session").unwrap())
        .await
        .unwrap();
    tokio::time::timeout(
        std::time::Duration::from_secs(3),
        viewer.stopped.cancelled(),
    )
    .await
    .unwrap();
    assert!(worker.stopped.is_cancelled());
}
