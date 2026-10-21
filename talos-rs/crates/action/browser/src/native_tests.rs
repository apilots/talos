#![cfg(unix)]
use super::*;
use crate::{BrowserInvocationIdentity, BrowserRuntime};
use std::sync::atomic::AtomicUsize;

#[derive(Default)]
struct Host {
    requests: Mutex<Vec<(BrowserTargetKind, BrowserHostOperation)>>,
    managed_generation: AtomicUsize,
}
impl BrowserHostPort for Host {
    fn available(&self) -> bool {
        true
    }
    fn request(
        self: Arc<Self>,
        request: BrowserHostRequest,
    ) -> BrowserFuture<'static, BrowserHostResponse> {
        Box::pin(async move {
            self.requests
                .lock()
                .unwrap()
                .push((request.target, request.operation.clone()));
            if matches!(request.operation, BrowserHostOperation::Shutdown) {
                self.managed_generation.fetch_add(1, Ordering::SeqCst);
                return Ok(BrowserHostResponse::Done);
            }
            let BrowserHostOperation::Execute {
                action,
                target_id,
                element,
                ..
            } = request.operation
            else {
                return Ok(BrowserHostResponse::Done);
            };
            let current = match request.target {
                BrowserTargetKind::Managed => {
                    (17 + self.managed_generation.load(Ordering::SeqCst)).to_string()
                }
                BrowserTargetKind::Connected => "chrome-tab".into(),
            };
            if target_id.as_deref().is_some_and(|id| id != current)
                || element.as_ref().is_some_and(|el| el.target_id != current)
            {
                return Err(TalosError::new(
                    ErrorCode::Conflict,
                    "Unknown browser target.",
                ));
            }
            Ok(BrowserHostResponse::Observation {
                page: BrowserHostPage {
                    target_id: current.clone(),
                    url: "https://example.test/".into(),
                    title: "Current browser".into(),
                    tabs: vec![],
                    text: None,
                },
                loader_id: Some("document".into()),
                elements: if matches!(action, BrowserAction::State) {
                    vec![BrowserHostElement {
                        backend_node_id: 1,
                        tag: "button".into(),
                        attributes: HashMap::new(),
                        target_id: current.clone(),
                        loader_id: "document".into(),
                    }]
                } else {
                    vec![]
                },
                screenshot: None,
                downloads: vec![],
            })
        })
    }
}
async fn invoke(
    runtime: &Arc<BrowserRuntime>,
    turn: &str,
    action: BrowserAction,
) -> TalosResult<McpCallToolResponse> {
    let lease = runtime.authorize(
        BrowserInvocationIdentity {
            user_id: "alice".into(),
            session_id: "session".into(),
            turn_id: turn.into(),
            call_id: Uuid::new_v4().to_string(),
        },
        &action,
    )?;
    runtime.invoke(lease.token(), action).await
}
fn targets(host: &Host) -> Vec<(BrowserTargetKind, Option<String>)> {
    host.requests
        .lock()
        .unwrap()
        .iter()
        .filter_map(|(kind, operation)| match operation {
            BrowserHostOperation::Execute { target_id, .. } => Some((*kind, target_id.clone())),
            _ => None,
        })
        .collect()
}
async fn element(runtime: &Arc<BrowserRuntime>, turn: &str) -> String {
    let result = invoke(runtime, turn, BrowserAction::State).await.unwrap();
    result.structured_content.unwrap()["elements"][0]["element"]
        .as_str()
        .unwrap()
        .into()
}

#[tokio::test]
async fn switching_browser_backends_resets_target_before_first_navigation() {
    let home = tempfile::tempdir().unwrap();
    let runtime = BrowserRuntime::new(
        home.path().into(),
        BrowserHostKind::Desktop,
        Arc::new(|| Ok(true)),
    )
    .unwrap();
    let host = Arc::new(Host::default());
    runtime
        .set_backend(NativeBrowser::new(host.clone()))
        .unwrap();
    for (turn, kind, name) in [
        ("first", BrowserTargetKind::Managed, "managed"),
        ("second", BrowserTargetKind::Connected, "connected"),
        ("third", BrowserTargetKind::Managed, "managed"),
    ] {
        runtime
            .operate(
                "alice".into(),
                BrowserOperation::SelectTarget {
                    target_id: name.into(),
                    expected_revision: runtime.snapshot().unwrap().control.revision,
                },
            )
            .await
            .unwrap();
        invoke(
            &runtime,
            turn,
            BrowserAction::Navigate {
                url: "https://example.test/".into(),
            },
        )
        .await
        .unwrap();
        let reference = element(&runtime, turn).await;
        invoke(
            &runtime,
            turn,
            BrowserAction::Click {
                element: reference.clone(),
            },
        )
        .await
        .unwrap();
        let segment = targets(&host);
        let id = match kind {
            BrowserTargetKind::Managed => "17",
            BrowserTargetKind::Connected => "chrome-tab",
        };
        assert_eq!(
            &segment[segment.len() - 3..],
            &[
                (kind, None),
                (kind, Some(id.into())),
                (kind, Some(id.into()))
            ]
        );
        runtime.finish_turn("session", turn);
    }
}

#[tokio::test]
async fn same_backend_retains_target_across_turns_but_expires_element_references() {
    let home = tempfile::tempdir().unwrap();
    let runtime = BrowserRuntime::new(
        home.path().into(),
        BrowserHostKind::Desktop,
        Arc::new(|| Ok(true)),
    )
    .unwrap();
    let host = Arc::new(Host::default());
    runtime
        .set_backend(NativeBrowser::new(host.clone()))
        .unwrap();
    let reference = element(&runtime, "first").await;
    runtime.finish_turn("session", "first");
    assert!(invoke(
        &runtime,
        "second",
        BrowserAction::Click { element: reference }
    )
    .await
    .unwrap_err()
    .is_code(ErrorCode::Conflict));
    invoke(
        &runtime,
        "second",
        BrowserAction::Navigate {
            url: "https://example.test/next".into(),
        },
    )
    .await
    .unwrap();
    assert_eq!(
        targets(&host),
        vec![
            (BrowserTargetKind::Managed, None),
            (BrowserTargetKind::Managed, Some("17".into()))
        ]
    );
}

#[tokio::test]
async fn enabling_after_shutdown_drops_the_closed_target_before_navigation() {
    let home = tempfile::tempdir().unwrap();
    let runtime = BrowserRuntime::new(
        home.path().into(),
        BrowserHostKind::Desktop,
        Arc::new(|| Ok(true)),
    )
    .unwrap();
    let host = Arc::new(Host::default());
    runtime
        .set_backend(NativeBrowser::new(host.clone()))
        .unwrap();
    element(&runtime, "before-disable").await;
    runtime.finish_turn("session", "before-disable");
    runtime.plugin_disabled().await.unwrap();
    assert_eq!(host.managed_generation.load(Ordering::SeqCst), 1);
    runtime.plugin_enabled().await.unwrap();
    invoke(
        &runtime,
        "after-enable",
        BrowserAction::Navigate {
            url: "https://example.test/reopened".into(),
        },
    )
    .await
    .unwrap();
    element(&runtime, "after-enable").await;
    assert_eq!(
        targets(&host),
        vec![
            (BrowserTargetKind::Managed, None),
            (BrowserTargetKind::Managed, None),
            (BrowserTargetKind::Managed, Some("18".into())),
        ]
    );
}
