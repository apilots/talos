use super::*;
use std::path::PathBuf;
use talos_types::talos::TalosConnectionMode;
use talos_types::{AgentUserContent, AgentUserInput};

fn start_turn_request(current_directory: Option<PathBuf>) -> StartTurnRequest {
    StartTurnRequest {
        node_id: None,
        session_id: "session-1".to_string(),
        input: AgentUserInput {
            content: vec![AgentUserContent::Text {
                text: "hello".to_string(),
            }],
        },
        model_selection: None,
        user_permission_mode: None,
        project_id: None,
        current_directory,
        client_request_id: None,
    }
}

#[test]
fn normalizes_embedded_environment_from_current_directory() {
    let directory = PathBuf::from("/tmp/talos-embedded");
    let request = start_turn_request(Some(directory.clone()));

    let (normalized, source) = prepare_embedded_start_turn(request).unwrap();

    assert_eq!(normalized.current_directory, Some(directory.clone()));
    let environment = source.environment.unwrap();
    assert_eq!(environment.directory, directory);
    assert_eq!(environment.connection_mode, TalosConnectionMode::Embedded);
}
