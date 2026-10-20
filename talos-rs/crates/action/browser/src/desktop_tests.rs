use super::*;
use crate::tests::Backend;
use std::sync::atomic::Ordering;

fn request(runtime: &BrowserRuntime, viewer_id: &str) -> WorkerDesktopOpen {
    WorkerDesktopOpen {
        binding: DesktopStreamBinding {
            stream_id: Uuid::new_v4().to_string(),
            viewer_id: viewer_id.into(),
            generation: runtime.snapshot().unwrap().control.generation,
            target_id: "managed".into(),
            input_mode: BrowserInputMode::ReadOnly,
            controller_id: None,
            input_epoch: None,
        },
        nonce: Uuid::new_v4().to_string(),
    }
}

#[tokio::test]
async fn failed_candidate_preserves_current_readonly_view_and_bounds_replacement() {
    let home = tempfile::tempdir().unwrap();
    let runtime = BrowserRuntime::new(
        home.path().into(),
        BrowserHostKind::Docker,
        Arc::new(|| Ok(true)),
    )
    .unwrap();
    let backend = Backend::new();
    backend.desktop.store(true, Ordering::SeqCst);
    runtime.set_backend(backend).unwrap();
    let viewer = Uuid::new_v4().to_string();
    let current = request(&runtime, &viewer);
    let current_stop = CancellationToken::new();
    runtime.state.lock().unwrap().views.insert(
        current.binding.stream_id.clone(),
        View {
            binding: current.binding.clone(),
            user_id: "alice".into(),
            stop: current_stop.clone(),
        },
    );
    let started = Arc::new(tokio::sync::Notify::new());
    let finish = Arc::new(tokio::sync::Notify::new());
    runtime
        .set_desktop_connector(Arc::new({
            let started = started.clone();
            let finish = finish.clone();
            move |_| {
                let started = started.clone();
                let finish = finish.clone();
                Box::pin(async move {
                    started.notify_one();
                    finish.notified().await;
                    Err(unavailable("Ingress rejected candidate."))
                })
            }
        }))
        .unwrap();
    let candidate = tokio::spawn({
        let runtime = runtime.clone();
        let candidate = request(&runtime, &viewer);
        async move { runtime.open_desktop("alice".into(), candidate).await }
    });
    started.notified().await;
    assert!(!current_stop.is_cancelled());
    assert!(runtime
        .open_desktop("alice".into(), request(&runtime, &viewer))
        .await
        .is_err());
    assert!(!current_stop.is_cancelled());
    finish.notify_one();
    assert!(candidate.await.unwrap().is_err());
    assert!(!current_stop.is_cancelled());
    assert_eq!(runtime.state.lock().unwrap().views.len(), 1);
    runtime.check_view(&current.binding).unwrap();
}

#[tokio::test]
async fn admitted_candidate_retires_previous_readonly_view() {
    let home = tempfile::tempdir().unwrap();
    let runtime = BrowserRuntime::new(
        home.path().into(),
        BrowserHostKind::Docker,
        Arc::new(|| Ok(true)),
    )
    .unwrap();
    let backend = Backend::new();
    backend.desktop.store(true, Ordering::SeqCst);
    runtime.set_backend(backend).unwrap();
    let viewer = Uuid::new_v4().to_string();
    let current = request(&runtime, &viewer);
    let current_stop = CancellationToken::new();
    runtime.state.lock().unwrap().views.insert(
        current.binding.stream_id.clone(),
        View {
            binding: current.binding.clone(),
            user_id: "alice".into(),
            stop: current_stop.clone(),
        },
    );
    runtime
        .set_desktop_connector(Arc::new(|_| {
            Box::pin(async {
                let (_sender, incoming) = mpsc::channel(1);
                let (outgoing, _receiver) = mpsc::channel(1);
                Ok(DesktopTransport {
                    incoming,
                    outgoing,
                    stopped: CancellationToken::new(),
                })
            })
        }))
        .unwrap();
    runtime
        .open_desktop("alice".into(), request(&runtime, &viewer))
        .await
        .unwrap();
    assert!(current_stop.is_cancelled());
    assert!(runtime.check_view(&current.binding).is_err());
}
