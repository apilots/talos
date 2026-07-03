use super::*;
use std::path::PathBuf;
use talos_types::talos::{TalosContext, TalosInputItem, TalosProvider, TalosSourceType};

fn start_turn_request(
    current_directory: Option<PathBuf>,
    environment: Option<TalosSessionEnvironment>,
) -> StartTurnRequest {
    StartTurnRequest {
        session_id: "session-1".to_string(),
        input: vec![TalosInputItem::Text {
            text: "hello".to_string(),
        }],
        workspace_id: None,
        context: Some(TalosContext {
            source_type: TalosSourceType::Cli,
            provider: TalosProvider::Cli,
            user_id: "user-1".to_string(),
            message_id: "message-1".to_string(),
            trace_id: "trace-1".to_string(),
            channel_id: None,
            thread_id: None,
            project_root: None,
            environment,
        }),
        model_ref: None,
        project_id: None,
        current_directory,
        client_request_id: None,
    }
}

#[test]
fn normalizes_embedded_environment_from_current_directory() {
    let directory = PathBuf::from("/tmp/talos-embedded");
    let request = start_turn_request(Some(directory.clone()), None);

    let normalized = normalize_embedded_start_turn_request(request).unwrap();

    assert_eq!(normalized.current_directory, Some(directory.clone()));
    let environment = normalized.context.unwrap().environment.unwrap();
    assert_eq!(environment.directory, directory);
    assert_eq!(environment.connection_mode, TalosConnectionMode::Embedded);
}

#[test]
fn overrides_existing_non_embedded_environment() {
    let environment = TalosSessionEnvironment {
        directory: PathBuf::from("/remote"),
        connection_mode: TalosConnectionMode::Remote,
    };
    let request = start_turn_request(None, Some(environment));

    let normalized = normalize_embedded_start_turn_request(request).unwrap();

    assert_eq!(normalized.current_directory, Some(PathBuf::from("/remote")));
    let environment = normalized.context.unwrap().environment.unwrap();
    assert_eq!(environment.directory, PathBuf::from("/remote"));
    assert_eq!(environment.connection_mode, TalosConnectionMode::Embedded);
}
