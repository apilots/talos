use super::*;
use crate::test_support::mock_model_info;

#[test]
fn selects_duplicate_model_id_by_provider_key() {
    let mut openai = mock_model_info("shared-model", 128_000);
    openai.provider = "openai".to_string();
    let mut ollama = mock_model_info("shared-model", 128_000);
    ollama.provider = "ollama".to_string();

    let selection = selection_for_key(&[openai, ollama], "ollama:shared-model", None)
        .expect("provider-qualified model should resolve");

    assert_eq!(selection.model_ref.provider, "ollama");
    assert_eq!(selection.model_ref.id, "shared-model");
}

#[test]
fn default_selection_honors_configured_reasoning_off() {
    let mut model = mock_model_info("model", 128_000);
    model.model.reasoning = true;
    model.model.request_parameters = serde_json::from_value(serde_json::json!({
        "chat_template_kwargs": { "enable_thinking": false }
    }))
    .unwrap();
    model.selection_capabilities = model.model.selection_capabilities();
    let selection = selection_for_key(&[model], "mock:model", None).unwrap();
    assert_eq!(selection.reasoning_effort, None);
}

#[tokio::test]
async fn picker_reasoning_off_is_kept_on_refresh_and_sent_on_first_turn() {
    use crate::test_support::{KeyCode, KeyEvent, KeyModifiers, MockGatewayClient, TestTuiAppExt};
    use crate::{GatewayClient, TuiApp};
    use std::sync::Arc;

    let client = Arc::new(MockGatewayClient::new());
    {
        let mut state = client.state.lock().unwrap();
        state.models[0].model.reasoning = true;
        state.models[0].model.request_parameters = serde_json::from_value(serde_json::json!({
            "enable_thinking": true
        }))
        .unwrap();
    }
    let gateway: Arc<dyn GatewayClient> = client.clone();
    let mut app = TuiApp::new_with_gateway_client(gateway);
    app.initialize_session().await;
    app.toggle_model_picker();
    for code in [KeyCode::Enter, KeyCode::Up, KeyCode::Up, KeyCode::Enter] {
        app.handle_input(KeyEvent::new(code, KeyModifiers::NONE))
            .await
            .unwrap();
    }
    let selected = app.current_model_selection().unwrap();
    assert_eq!(selected.reasoning_effort, None);
    assert!(app.current_model_status_label().ends_with("none"));
    app.refresh_models_from_gateway().await.unwrap();
    assert_eq!(app.current_model_selection(), Some(selected.clone()));
    app.send_user_message("configured controls".to_string())
        .await
        .unwrap();
    for _ in 0..32 {
        tokio::task::yield_now().await;
        if !client.state.lock().unwrap().sent_requests.is_empty() {
            break;
        }
    }
    let sent = client.state.lock().unwrap().sent_requests.clone();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].model_selection.as_ref(), Some(&selected));
}
