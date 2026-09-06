use talos_sdk::{AgentUserContent, AgentUserInput, ModelRef, ModelSelection, StartTurnRequest};

pub(super) fn start_turn_request(
    session_id: &str,
    text: &str,
    client_request_id: Option<String>,
) -> StartTurnRequest {
    StartTurnRequest {
        session_id: session_id.to_string(),
        input: AgentUserInput {
            content: vec![AgentUserContent::Text {
                text: text.to_string(),
            }],
        },
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
