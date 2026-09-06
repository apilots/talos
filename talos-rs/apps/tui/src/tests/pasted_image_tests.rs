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
