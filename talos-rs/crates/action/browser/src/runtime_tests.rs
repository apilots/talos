use super::*;
use crate::BrowserActionExt;
use serde_json::json;
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc,
};
use std::time::Duration;
use talos_error::ErrorCode;
use talos_types::*;

pub(super) struct Backend {
    sends: AtomicUsize,
    fail: AtomicBool,
    first: tokio::sync::Notify,
    pub(super) next: tokio::sync::Notify,
    pub(super) desktop: AtomicBool,
    hold_cleanup: AtomicBool,
    cleanup_started: tokio::sync::Notify,
    cleanup_continue: tokio::sync::Notify,
}
impl Backend {
    pub(super) fn new() -> Arc<Self> {
        Arc::new(Self {
            sends: AtomicUsize::new(0),
            fail: AtomicBool::new(false),
            first: Default::default(),
            next: Default::default(),
            desktop: AtomicBool::new(false),
            hold_cleanup: AtomicBool::new(false),
            cleanup_started: Default::default(),
            cleanup_continue: Default::default(),
        })
    }
}
impl BrowserBackend for Backend {
    fn desktop_available(&self) -> bool {
        self.desktop.load(Ordering::SeqCst)
    }
    fn desktop(&self) -> BrowserFuture<'_, tokio::net::TcpStream> {
        Box::pin(async {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            Ok(
                tokio::net::TcpStream::connect(listener.local_addr().unwrap())
                    .await
                    .unwrap(),
            )
        })
    }
    fn readiness(&self, _: BrowserTargetKind) -> BrowserReadiness {
        BrowserReadiness::Ready
    }
    fn execute(
        &self,
        _: BrowserAction,
        dispatch: BrowserDispatch,
    ) -> BrowserFuture<'_, McpCallToolResponse> {
        Box::pin(async move {
            if self.fail.load(Ordering::SeqCst) {
                return Err(TalosError::internal_with_source(
                    "Browser execution failed.",
                    std::io::Error::other("private-backend-diagnostic"),
                ));
            }
            {
                let _input = dispatch.input_permit().await?;
                self.sends.fetch_add(1, Ordering::SeqCst);
            }
            self.first.notify_one();
            self.next.notified().await;
            {
                let _input = dispatch.input_permit().await?;
                self.sends.fetch_add(1, Ordering::SeqCst);
            }
            Ok(McpCallToolResponse {
                content: vec![],
                is_error: false,
                structured_content: None,
            })
        })
    }
    fn connect(&self, _user: String) -> BrowserFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }
    fn disconnect(&self, _user: String) -> BrowserFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }
    fn open_local(&self, _user: String) -> BrowserFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }
    fn release_inputs(&self) -> BrowserFuture<'_, ()> {
        Box::pin(async {
            if self.hold_cleanup.swap(false, Ordering::SeqCst) {
                self.cleanup_started.notify_one();
                self.cleanup_continue.notified().await;
            }
            Ok(())
        })
    }
    fn shutdown(&self) -> BrowserFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }
}

#[tokio::test]
async fn backend_readiness_cannot_bypass_platform_private_ipc_admission() {
    let home = tempfile::tempdir().unwrap();
    let runtime = BrowserRuntime::new(
        home.path().into(),
        BrowserHostKind::Desktop,
        Arc::new(|| Ok(true)),
    )
    .unwrap();
    runtime.set_backend(Backend::new()).unwrap();
    let snapshot = runtime
        .operate("alice".into(), BrowserOperation::Read)
        .await
        .unwrap();
    assert_eq!(snapshot.targets.len(), 2);
    let admitted = runtime.authorize(
        BrowserInvocationIdentity {
            user_id: "alice".into(),
            session_id: "session".into(),
            turn_id: "turn".into(),
            call_id: "call".into(),
        },
        &BrowserAction::Navigate {
            url: "https://example.com".into(),
        },
    );
    #[cfg(unix)]
    {
        assert!(snapshot
            .targets
            .iter()
            .all(|target| target.readiness == BrowserReadiness::Ready));
        assert!(admitted.is_ok());
    }
    #[cfg(not(unix))]
    {
        assert!(snapshot.targets.iter().all(|target| matches!(
            &target.readiness,
            BrowserReadiness::Unavailable { error }
                if error.code == ErrorCode::ServiceUnavailable
        )));
        assert!(admitted
            .err()
            .unwrap()
            .is_code(ErrorCode::ServiceUnavailable));
        assert_eq!(
            runtime.snapshot().unwrap().control.owner,
            BrowserControlOwner::Idle
        );
        assert!(runtime
            .operate("alice".into(), BrowserOperation::OpenLocal)
            .await
            .unwrap_err()
            .is_code(ErrorCode::ServiceUnavailable));
        let selected = runtime
            .operate(
                "alice".into(),
                BrowserOperation::SelectTarget {
                    target_id: "connected".into(),
                    expected_revision: snapshot.control.revision,
                },
            )
            .await
            .unwrap();
        assert!(runtime
            .operate(
                "alice".into(),
                BrowserOperation::Connect {
                    expected_revision: selected.control.revision,
                },
            )
            .await
            .unwrap_err()
            .is_code(ErrorCode::ServiceUnavailable));
    }
}

#[tokio::test]
async fn newer_pause_wins_over_waiting_takeover_and_release() {
    for release in [false, true] {
        let home = tempfile::tempdir().unwrap();
        let runtime = BrowserRuntime::new(
            home.path().into(),
            BrowserHostKind::Docker,
            Arc::new(|| Ok(true)),
        )
        .unwrap();
        let backend = Backend::new();
        backend.desktop.store(true, Ordering::SeqCst);
        runtime.set_backend(backend.clone()).unwrap();
        let operation = if release {
            let handoff = runtime
                .operate(
                    "alice".into(),
                    BrowserOperation::TakeControl {
                        expected_revision: runtime.snapshot().unwrap().control.revision,
                    },
                )
                .await
                .unwrap();
            let BrowserControlOwner::HandingOver { controller_id } = handoff.control.owner else {
                panic!("handoff");
            };
            BrowserOperation::ReleaseControl {
                controller_id,
                expected_revision: handoff.control.revision,
            }
        } else {
            BrowserOperation::TakeControl {
                expected_revision: runtime.snapshot().unwrap().control.revision,
            }
        };
        backend.hold_cleanup.store(true, Ordering::SeqCst);
        let earlier = {
            let runtime = runtime.clone();
            tokio::spawn(async move { runtime.operate("alice".into(), operation).await })
        };
        tokio::time::timeout(Duration::from_secs(1), backend.cleanup_started.notified())
            .await
            .unwrap();
        let old_revision = runtime.snapshot().unwrap().control.revision;
        let later = {
            let runtime = runtime.clone();
            tokio::spawn(async move {
                runtime
                    .operate(
                        "alice".into(),
                        BrowserOperation::Pause {
                            expected_revision: old_revision,
                        },
                    )
                    .await
            })
        };
        tokio::time::timeout(Duration::from_secs(1), async {
            while runtime.snapshot().unwrap().control.revision == old_revision {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        backend.cleanup_continue.notify_one();
        let earlier = tokio::time::timeout(Duration::from_secs(1), earlier)
            .await
            .unwrap()
            .unwrap();
        assert!(earlier.unwrap_err().is_code(ErrorCode::Conflict));
        let latest = tokio::time::timeout(Duration::from_secs(1), later)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(matches!(
            latest.control.owner,
            BrowserControlOwner::Suspended { .. }
        ));
        assert!(runtime
            .authorize(
                identity(),
                &BrowserAction::Navigate {
                    url: "https://example.com".into(),
                }
            )
            .is_err());
    }
}
fn identity() -> BrowserInvocationIdentity {
    BrowserInvocationIdentity {
        user_id: "alice".into(),
        session_id: "session".into(),
        turn_id: "turn".into(),
        call_id: "call".into(),
    }
}
fn runtime(home: &std::path::Path) -> Arc<BrowserRuntime> {
    BrowserRuntime::new(home.into(), BrowserHostKind::Desktop, Arc::new(|| Ok(true))).unwrap()
}

#[tokio::test]
async fn pause_fences_each_transport_step_and_requires_explicit_resume_after_restart() {
    let home = tempfile::tempdir().unwrap();
    let runtime = runtime(home.path());
    let backend = Backend::new();
    runtime.set_backend(backend.clone()).unwrap();
    let action = BrowserAction::Navigate {
        url: "https://example.com".into(),
    };
    let lease = runtime.authorize(identity(), &action).unwrap();
    let running = {
        let runtime = runtime.clone();
        let token = lease.token().to_owned();
        let action = action.clone();
        tokio::spawn(async move { runtime.invoke(&token, action).await })
    };
    backend.first.notified().await;
    let revision = runtime.snapshot().unwrap().control.revision;
    runtime
        .operate(
            "user".into(),
            BrowserOperation::Pause {
                expected_revision: revision,
            },
        )
        .await
        .unwrap();
    backend.next.notify_one();
    assert!(running.await.unwrap().is_err());
    assert_eq!(backend.sends.load(Ordering::SeqCst), 1);
    assert!(runtime
        .authorize(identity(), &action)
        .unwrap_err()
        .is_code(ErrorCode::Conflict));
    let restarted = BrowserRuntime::new(
        home.path().into(),
        BrowserHostKind::Desktop,
        Arc::new(|| Ok(true)),
    )
    .unwrap();
    assert!(matches!(
        restarted.snapshot().unwrap().control.owner,
        BrowserControlOwner::Suspended { .. }
    ));
    let revision = restarted.snapshot().unwrap().control.revision;
    restarted
        .operate(
            "user".into(),
            BrowserOperation::Resume {
                expected_revision: revision,
            },
        )
        .await
        .unwrap();
    assert!(matches!(
        restarted.snapshot().unwrap().control.owner,
        BrowserControlOwner::Idle
    ));
}

#[tokio::test]
async fn capability_is_bound_to_arguments_single_use_turn_and_cancellation() {
    let home = tempfile::tempdir().unwrap();
    let runtime = runtime(home.path());
    let backend = Backend::new();
    runtime.set_backend(backend.clone()).unwrap();
    let action = BrowserAction::State;
    let lease = runtime.authorize(identity(), &action).unwrap();
    let changed = BrowserAction::Navigate {
        url: "https://example.com".into(),
    };
    assert!(runtime
        .invoke(lease.token(), changed)
        .await
        .unwrap_err()
        .is_code(ErrorCode::PermissionDenied));
    let running = {
        let runtime = runtime.clone();
        let token = lease.token().to_owned();
        tokio::spawn(async move { runtime.invoke(&token, BrowserAction::State).await })
    };
    backend.first.notified().await;
    assert!(runtime
        .invoke(lease.token(), action)
        .await
        .unwrap_err()
        .is_code(ErrorCode::PermissionDenied));
    drop(lease);
    assert!(running
        .await
        .unwrap()
        .unwrap_err()
        .is_code(ErrorCode::Cancelled));
    assert_eq!(backend.sends.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn native_rejects_remote_control_and_stale_settings_changes() {
    let home = tempfile::tempdir().unwrap();
    let runtime = runtime(home.path());
    assert!(matches!(
        runtime.snapshot().unwrap().display,
        BrowserDisplayCapability::Unavailable { .. }
    ));
    assert!(runtime
        .operate(
            "user".into(),
            BrowserOperation::TakeControl {
                expected_revision: 0
            }
        )
        .await
        .unwrap_err()
        .is_code(ErrorCode::InvalidRequest));
    runtime
        .operate(
            "user".into(),
            BrowserOperation::SelectTarget {
                target_id: "connected".into(),
                expected_revision: 0,
            },
        )
        .await
        .unwrap();
    assert!(runtime
        .operate(
            "user".into(),
            BrowserOperation::Pause {
                expected_revision: 0
            }
        )
        .await
        .unwrap_err()
        .is_code(ErrorCode::Conflict));
    assert!(matches!(
        runtime.snapshot().unwrap().control.owner,
        BrowserControlOwner::Idle
    ));
}

#[tokio::test]
async fn canonical_plugin_disable_and_turn_settlement_revoke_invocations() {
    let home = tempfile::tempdir().unwrap();
    let enabled = Arc::new(AtomicBool::new(true));
    let read = enabled.clone();
    let runtime = BrowserRuntime::new(
        home.path().into(),
        BrowserHostKind::Desktop,
        Arc::new(move || Ok(read.load(Ordering::SeqCst))),
    )
    .unwrap();
    let action = BrowserAction::Key {
        key: "Enter".into(),
    };
    let lease = runtime.authorize(identity(), &action).unwrap();
    let mut other = identity();
    other.user_id = "bob".into();
    assert!(runtime
        .authorize(other, &action)
        .unwrap_err()
        .is_code(ErrorCode::Conflict));
    runtime.finish_turn("session", "turn");
    assert!(runtime
        .invoke(lease.token(), action.clone())
        .await
        .unwrap_err()
        .is_code(ErrorCode::PermissionDenied));
    enabled.store(false, Ordering::SeqCst);
    assert!(!runtime.snapshot().unwrap().enabled);
    assert!(runtime.authorize(identity(), &action).is_err());
}

#[test]
fn tool_arguments_cannot_supply_identity_or_arbitrary_protocol_commands() {
    assert!(BrowserAction::parse(
        "browser_click",
        json!({"element":"doc:1","sessionId":"other"})
    )
    .is_err());
    assert!(BrowserAction::parse("browser_navigate", json!({"url":"file:///etc/passwd"})).is_err());
    assert!(BrowserAction::parse("browser_state", json!({"kind":"navigate"})).is_err());
    assert!(BrowserAction::parse("browser_eval", json!({"code":"process.exit()"})).is_err());
    assert_eq!(BrowserAction::catalog().len(), 10);
}

#[tokio::test]
async fn account_logout_revokes_late_read_calls_until_explicit_resume() {
    let home = tempfile::tempdir().unwrap();
    let runtime = runtime(home.path());
    runtime.set_backend(Backend::new()).unwrap();
    let lease = runtime
        .authorize(identity(), &BrowserAction::State)
        .unwrap();
    runtime.reset_account("alice").await.unwrap();
    assert!(runtime
        .invoke(lease.token(), BrowserAction::State)
        .await
        .is_err());
    assert!(runtime
        .authorize(identity(), &BrowserAction::State)
        .is_err());
    let revision = runtime.snapshot().unwrap().control.revision;
    runtime
        .operate(
            "alice".into(),
            BrowserOperation::Resume {
                expected_revision: revision,
            },
        )
        .await
        .unwrap();
    assert!(runtime.authorize(identity(), &BrowserAction::State).is_ok());
}

#[cfg(unix)]
#[tokio::test]
async fn ipc_reports_original_source_once_and_shutdown_waits_for_private_socket_cleanup() {
    let home = tempfile::tempdir().unwrap();
    let log = home.path().join("diagnostic.log");
    let guard = talos_logging::init_with_config(talos_logging::LoggingConfig {
        file: Some(log.to_string_lossy().into()),
        stdout: false,
        rotation: None, // Keep diagnostic-only fixtures on their explicit single-file sink.
        ..Default::default()
    })
    .unwrap();
    let runtime = runtime(home.path());
    let backend = Backend::new();
    backend.fail.store(true, Ordering::SeqCst);
    runtime.set_backend(backend).unwrap();
    let socket = runtime.start_ipc(home.path()).await.unwrap();
    let parent = socket.parent().unwrap().to_path_buf();
    let action = BrowserAction::State;
    let lease = runtime.authorize(identity(), &action).unwrap();
    let error = crate::ipc::call(
        &socket,
        lease.token().into(),
        "browser_state".into(),
        json!({}),
    )
    .await
    .unwrap_err();
    assert!(!serde_json::to_string(&error.to_api_error())
        .unwrap()
        .contains("private-backend-diagnostic"));
    runtime.shutdown().await.unwrap();
    assert!(!socket.exists());
    assert!(!parent.exists());
    assert!(tokio::net::UnixStream::connect(&socket).await.is_err());
    drop(guard);
    let diagnostic = std::fs::read_to_string(log).unwrap();
    assert_eq!(diagnostic.matches("private-backend-diagnostic").count(), 1);
}
