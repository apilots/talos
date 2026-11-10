use std::sync::Arc;

use bytes::Bytes;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use talos_sdk::{AgentUserContent, AgentUserInput};

use crate::client::GatewayClient;
use crate::test_support::MockGatewayClient;
use crate::TuiApp;

#[tokio::test]
async fn pasted_image_uploads_and_sends_only_ordered_file_ids() {
    let client = Arc::new(MockGatewayClient::new());
    let state = client.state.clone();
    state.lock().unwrap().models[0].model.input = vec![
        talos_sdk::ModelInputKind::Text,
        talos_sdk::ModelInputKind::Image,
    ];
    let gateway_client: Arc<dyn GatewayClient> = client;
    let mut app = TuiApp::new_with_gateway_client(gateway_client);
    app.initialize_session().await;
    app.current_project_id = Some("project-1".to_string());
    app.handle_pasted_image(Bytes::from_static(b"png-one"))
        .unwrap();

    assert_eq!(app.input_buffer, "[Image #1]");
    assert_eq!(app.pasted_image_attachments.len(), 1);

    app.submit_input().await.unwrap();
    wait_for_request(&mut app, &state).await;

    let state = state.lock().unwrap();
    assert_eq!(state.upload_requests.len(), 1);
    assert_eq!(state.upload_requests[0].0.filename, "pasted-image-1.png");
    assert_eq!(
        state.upload_requests[0].0.scope,
        talos_sdk::FileScope::Project {
            project_id: "project-1".to_string(),
        }
    );
    assert_eq!(state.upload_requests[0].1, Bytes::from_static(b"png-one"));
    assert_eq!(
        state.sent_requests[0].input,
        AgentUserInput {
            content: vec![AgentUserContent::File {
                file_id: "uploaded-file-1".to_string(),
            }],
        }
    );
    assert_eq!(
        state.sent_requests[0].project_id.as_deref(),
        Some("project-1")
    );
}

#[tokio::test]
async fn pasted_image_upload_order_follows_marker_order() {
    let client = Arc::new(MockGatewayClient::new());
    let state = client.state.clone();
    state.lock().unwrap().models[0].model.input = vec![
        talos_sdk::ModelInputKind::Text,
        talos_sdk::ModelInputKind::Image,
    ];
    let gateway_client: Arc<dyn GatewayClient> = client;
    let mut app = TuiApp::new_with_gateway_client(gateway_client);
    app.initialize_session().await;
    app.handle_pasted_image(Bytes::from_static(b"png-one"))
        .unwrap();
    app.cursor_index = 0;
    app.handle_pasted_image(Bytes::from_static(b"png-two"))
        .unwrap();

    app.submit_input().await.unwrap();
    wait_for_request(&mut app, &state).await;

    let state = state.lock().unwrap();
    assert_eq!(
        state
            .upload_requests
            .iter()
            .map(|(_, bytes)| bytes.clone())
            .collect::<Vec<_>>(),
        vec![
            Bytes::from_static(b"png-two"),
            Bytes::from_static(b"png-one"),
        ]
    );
    assert_eq!(
        state.sent_requests[0].input,
        AgentUserInput {
            content: vec![
                AgentUserContent::File {
                    file_id: "uploaded-file-1".to_string(),
                },
                AgentUserContent::File {
                    file_id: "uploaded-file-2".to_string(),
                },
            ],
        }
    );
}

#[tokio::test]
async fn pasted_image_marker_edits_are_atomic() {
    let client = Arc::new(MockGatewayClient::new());
    client.state.lock().unwrap().models[0].model.input = vec![
        talos_sdk::ModelInputKind::Text,
        talos_sdk::ModelInputKind::Image,
    ];
    let gateway_client: Arc<dyn GatewayClient> = client;
    let mut app = TuiApp::new_with_gateway_client(gateway_client);
    app.initialize_session().await;
    app.handle_pasted_image(Bytes::from_static(b"png")).unwrap();
    let marker = app.input_buffer.clone();

    app.handle_input(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE))
        .await
        .unwrap();
    assert_eq!(app.cursor_index, 0);
    app.handle_input(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE))
        .await
        .unwrap();
    assert_eq!(app.cursor_index, marker.chars().count());

    app.cursor_index = 3;
    app.insert_char('x');
    assert_eq!(app.input_buffer, format!("{marker}x"));

    app.cursor_index = 3;
    app.handle_backspace();
    assert_eq!(app.input_buffer, "x");
    assert!(app.pasted_image_attachments.is_empty());
}

#[tokio::test]
async fn pasted_image_failure_keeps_draft_and_reuses_successful_uploads() {
    let client = Arc::new(MockGatewayClient::new());
    let state = client.state.clone();
    {
        let mut state = state.lock().unwrap();
        state.models[0].model.input = vec![
            talos_sdk::ModelInputKind::Text,
            talos_sdk::ModelInputKind::Image,
        ];
        state.upload_failure_calls = vec![2];
    }
    let gateway_client: Arc<dyn GatewayClient> = client;
    let mut app = TuiApp::new_with_gateway_client(gateway_client);
    app.initialize_session().await;
    app.handle_pasted_image(Bytes::from_static(b"png-one"))
        .unwrap();
    app.handle_pasted_image(Bytes::from_static(b"png-two"))
        .unwrap();

    app.submit_input().await.unwrap();
    wait_for_image_upload(&mut app).await;

    assert_eq!(state.lock().unwrap().upload_requests.len(), 2);
    assert!(state.lock().unwrap().sent_requests.is_empty());
    assert_eq!(app.input_buffer, "[Image #1][Image #2]");
    assert!(matches!(
        app.pasted_image_attachments[0].state,
        crate::types::PastedImageState::Uploaded { .. }
    ));
    assert!(matches!(
        app.pasted_image_attachments[1].state,
        crate::types::PastedImageState::Pending { .. }
    ));

    app.submit_input().await.unwrap();
    wait_for_request(&mut app, &state).await;

    let state = state.lock().unwrap();
    assert_eq!(state.upload_requests.len(), 3);
    assert_eq!(
        state.sent_requests[0].input,
        AgentUserInput {
            content: vec![
                AgentUserContent::File {
                    file_id: "uploaded-file-1".to_string(),
                },
                AgentUserContent::File {
                    file_id: "uploaded-file-3".to_string(),
                },
            ],
        }
    );
}

#[tokio::test]
async fn backspace_removes_a_pasted_image_marker_and_attachment() {
    let client = Arc::new(MockGatewayClient::new());
    client.state.lock().unwrap().models[0].model.input = vec![
        talos_sdk::ModelInputKind::Text,
        talos_sdk::ModelInputKind::Image,
    ];
    let gateway_client: Arc<dyn GatewayClient> = client;
    let mut app = TuiApp::new_with_gateway_client(gateway_client);
    app.initialize_session().await;
    app.handle_pasted_image(Bytes::from_static(b"png")).unwrap();

    app.handle_input(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE))
        .await
        .unwrap();

    assert!(app.input_buffer.is_empty());
    assert!(app.pasted_image_attachments.is_empty());
}

async fn wait_for_image_upload(app: &mut TuiApp) {
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while app.pending_image_upload.is_some() {
            app.poll_image_upload().await;
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("image upload settled");
}

async fn wait_for_request(
    app: &mut TuiApp,
    state: &Arc<std::sync::Mutex<crate::test_support::MockGatewayState>>,
) {
    for _ in 0..10 {
        app.on_tick().await;
        tokio::task::yield_now().await;
        if !state.lock().unwrap().sent_requests.is_empty() {
            break;
        }
    }
}

#[tokio::test]
async fn held_upload_and_recovery_allow_input_and_timeline_progress() {
    let client = Arc::new(MockGatewayClient::new());
    let entered = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let recovery_entered = Arc::new(tokio::sync::Notify::new());
    let recovery_release = Arc::new(tokio::sync::Notify::new());
    {
        let mut state = client.state.lock().unwrap();
        state.models[0]
            .model
            .input
            .push(talos_sdk::ModelInputKind::Image);
        state.upload_gate = Some((entered.clone(), release.clone()));
        state.reconnect_gate = Some((recovery_entered.clone(), recovery_release));
        state.active_turn_id = Some("turn-active".into());
    }
    let mut app = TuiApp::new_with_gateway_client(client.clone());
    app.initialize_session().await;
    app.session_id = Some("sess-test".into());
    let (timeline, events) = tokio::sync::mpsc::channel(3);
    app.pending_stream = Some(events);
    app.in_flight = true;
    app.active_message = Some("Working".into());
    app.last_stream_event_at = Some(
        std::time::Instant::now()
            - crate::STREAM_STALL_TIMEOUT
            - std::time::Duration::from_millis(1),
    );
    app.handle_pasted_image(Bytes::from_static(b"png")).unwrap();

    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        app.on_tick().await;
        recovery_entered.notified().await;
        assert_eq!(
            client.get_session("sess-test").await.unwrap().id,
            "sess-test"
        );
        app.handle_input(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .await
            .unwrap();
        entered.notified().await;
        app.handle_input(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE))
            .await
            .unwrap();
        app.handle_input(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
            .await
            .unwrap();
        for event in [
            crate::test_support::turn_started_event("sess-test", 1),
            crate::test_support::assistant_block_started_event("message", "sess-test", 2),
            crate::test_support::assistant_text_delta_event(
                "message",
                "sess-test",
                3,
                "live progress",
            ),
        ] {
            timeline.send(event).await.unwrap();
        }
        while app.trace_event_count == 0
            || client.state.lock().unwrap().cancelled_sessions.is_empty()
        {
            app.on_tick().await;
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(app.input_buffer, "[Image #1]x");
    assert_eq!(app.trace_event_count, 3);
    assert!(crate::test_support::interactive_transcript_text(&app).contains("live progress"));
    assert!(app.pending_reconnect.is_none());
    assert!(client.state.lock().unwrap().sent_requests.is_empty());

    release.notify_one();
    wait_for_image_upload(&mut app).await;
    assert!(app.pending_image_upload.is_none());
    assert_eq!(app.input_buffer, "[Image #1]x");
    assert!(matches!(
        app.pasted_image_attachments[0].state,
        crate::types::PastedImageState::Uploaded { .. }
    ));
    assert!(client.state.lock().unwrap().sent_requests.is_empty());
}
