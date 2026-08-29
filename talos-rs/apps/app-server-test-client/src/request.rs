use talos_sdk::{ModelRef, ModelSelection, StartTurnRequest, TalosInputItem};

use super::test_talos_context;

pub(super) fn start_turn_request(
    session_id: &str,
    text: &str,
    client_request_id: Option<String>,
) -> StartTurnRequest {
    StartTurnRequest {
        session_id: session_id.to_string(),
        input: vec![TalosInputItem::Text {
            text: text.to_string(),
        }],
        context: Some(test_talos_context()),
        model_selection: test_model_ref().map(ModelSelection::without_reasoning),
        user_permission_mode: None,
        project_id: None,
        current_directory: None,
        client_request_id,
    }
}

pub(super) fn test_model_ref() -> Option<ModelRef> {
    let provider = std::env::var("TALOS_APP_SERVER_TEST_PROVIDER")
        .ok()
        .filter(|value| !value.trim().is_empty())?;
    let id = std::env::var("TALOS_APP_SERVER_TEST_MODEL")
        .ok()
        .filter(|value| !value.trim().is_empty())?;
    Some(ModelRef { provider, id })
}
