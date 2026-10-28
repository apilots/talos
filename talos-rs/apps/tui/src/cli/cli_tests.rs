use super::*;
use std::collections::{HashMap, HashSet};
use talos_sdk::{
    AgentAssistantText, AgentAssistantTextPhase, AgentEvent, AgentEventPayload, AgentMessageBlock,
    AgentMessageBlockCompleted, AgentMessageBlockContent, AgentMessageBlockStatus, AgentStopReason,
    AgentTurnCompleted, AgentTurnFailed, AgentTurnRef, ErrorCode, TimelineEvent,
};

#[test]
fn parses_exec_command() {
    assert!(Cli::try_parse_from(["talos", "exec", "hello"]).is_ok());
}

#[test]
fn parses_tui_default_command() {
    assert!(Cli::try_parse_from(["talos"]).is_ok());
}

#[test]
fn render_event_keeps_exec_open_after_the_final_answer_block_completes() {
    let mut phases = HashMap::from([("final-1".to_string(), AgentAssistantTextPhase::FinalAnswer)]);
    let mut texts = HashMap::new();
    let mut written = HashSet::new();
    let mut output = String::new();
    let mut stdout = std::io::stdout();
    let event = timeline_event(AgentEventPayload::MessageBlockCompleted(
        AgentMessageBlockCompleted {
            message_id: "message-1".to_string(),
            block: AgentMessageBlock {
                block_id: "final-1".to_string(),
                status: AgentMessageBlockStatus::Completed,
                content: AgentMessageBlockContent::AssistantText(AgentAssistantText {
                    phase: AgentAssistantTextPhase::FinalAnswer,
                    text: "done".to_string(),
                }),
                started_at: 1,
                completed_at: Some(1),
            },
        },
    ));

    let should_finish = render_event(
        &event,
        &mut phases,
        &mut texts,
        &mut written,
        &mut output,
        &mut stdout,
    )
    .expect("final answer should render");

    assert!(!should_finish);
    assert_eq!(output, "done");
}

#[test]
fn render_event_finishes_exec_on_turn_completed() {
    let should_finish = render(AgentEventPayload::TurnCompleted(AgentTurnCompleted {
        stop_reason: AgentStopReason::EndTurn,
    }))
    .expect("turn completion should finish");

    assert!(should_finish);
}

#[test]
fn render_event_returns_the_canonical_turn_failure() {
    let error = render(AgentEventPayload::TurnFailed(AgentTurnFailed {
        error: talos_error::ApiError {
            code: ErrorCode::ProviderUnavailable,
            message: "provider request failed".to_string(),
            localization: None,
        },
        retryable: true,
    }))
    .expect_err("turn failure should fail exec mode");

    assert_eq!(error.to_string(), "provider request failed");
}

fn render(payload: AgentEventPayload) -> Result<bool, Box<dyn std::error::Error>> {
    render_event(
        &timeline_event(payload),
        &mut HashMap::new(),
        &mut HashMap::new(),
        &mut HashSet::new(),
        &mut String::new(),
        &mut std::io::stdout(),
    )
}

fn timeline_event(payload: AgentEventPayload) -> TimelineEvent {
    TimelineEvent {
        event_id: "event-1".to_string(),
        event_seq: 1,
        persisted_at: 1,
        event: talos_sdk::TimelineEventPayload::Agent(AgentEvent {
            identity: AgentTurnRef::new("session-1", "turn-1"),
            sequence: 1,
            occurred_at: 1,
            payload,
        }),
    }
}

#[test]
fn plugin_install_keeps_target_commit_and_explicit_install_policy() {
    let cli = Cli::try_parse_from([
        "talos",
        "plugin",
        "install",
        "plugin-browser",
        "--node",
        "computer-1",
        "--commit",
        "fixed-revision",
        "--disabled",
        "--replace",
    ])
    .unwrap();
    let Some(Commands::Plugin(types::PluginCmd { node, command })) = cli.command else {
        panic!("expected Plugin command");
    };
    assert_eq!(node.as_deref(), Some("computer-1"));
    assert!(matches!(command, types::PluginCommands::Install {
        plugin_id, commit: Some(commit), disabled: true, replace: true,
    } if plugin_id == "plugin-browser" && commit == "fixed-revision"));
}

#[test]
fn plugin_lifecycle_uses_install_ids_and_preserves_the_selected_computer() {
    for operation in ["enable", "disable", "update", "uninstall"] {
        let cli = Cli::try_parse_from([
            "talos",
            "plugin",
            "--node",
            "computer-2",
            operation,
            "install-existing",
        ])
        .unwrap();
        let Some(Commands::Plugin(types::PluginCmd { node, command })) = cli.command else {
            panic!("expected Plugin command");
        };
        assert_eq!(node.as_deref(), Some("computer-2"));
        let install_id = match command {
            types::PluginCommands::Enable { install_id }
            | types::PluginCommands::Disable { install_id }
            | types::PluginCommands::Update { install_id, .. }
            | types::PluginCommands::Uninstall { install_id } => install_id,
            _ => panic!("expected existing installation command"),
        };
        assert_eq!(install_id, "install-existing");
    }
    let cli = Cli::try_parse_from(["talos", "plugin", "list"]).unwrap();
    assert!(matches!(
        cli.command,
        Some(Commands::Plugin(types::PluginCmd {
            node: None,
            command: types::PluginCommands::List,
        }))
    ));
}
