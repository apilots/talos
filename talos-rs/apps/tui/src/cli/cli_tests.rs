use super::*;
use std::collections::{HashMap, HashSet};
use talos_sdk::{
    AgentAssistantText, AgentAssistantTextPhase, AgentEvent, AgentEventPayload, AgentMessageBlock,
    AgentMessageBlockCompleted, AgentMessageBlockContent, AgentMessageBlockStatus, AgentRunMetrics,
    AgentStopReason, AgentTurnCompleted, AgentTurnFailed, AgentTurnRef, ErrorCode, TimelineEvent,
    TokenUsage,
};

#[test]
fn exec_drives_only_the_session_it_holds_and_prints_a_waiting_contract() {
    let cli = Cli::try_parse_from(["talos", "exec", "hello"]).unwrap();
    let Some(Commands::Exec(cmd)) = cli.command else {
        panic!("exec command");
    };
    let policy = session_ownership(&TerminalMode::Command(Commands::Exec(cmd)));
    assert_eq!(policy.hold, talos_base::SessionHold::Process);
    assert_eq!(policy.kind, talos_error::SessionOwnerKind::Exec);

    let pending = AgentPendingInput::Message {
        action_id: "action-1".into(),
        prompt: "Need a choice".into(),
    };
    let line = waiting_input_line("session-1", "turn-1", &pending);
    assert!(line.starts_with(
        "WAITING_FOR_INPUT session=session-1 turn=turn-1 kind=message action_id=action-1 request="
    ));
    let request = line.split_once("request=").unwrap().1;
    let parsed: serde_json::Value = serde_json::from_str(request).unwrap();
    assert_eq!(parsed["type"], "message");
    assert_eq!(parsed["actionId"], "action-1");
    assert_eq!(parsed["prompt"], "Need a choice");

    let status: serde_json::Value =
        serde_json::from_str(&exec_result_line("session-1", "turn-1", "waiting")).unwrap();
    assert_eq!(
        status,
        serde_json::json!({
            "sessionId": "session-1",
            "turnId": "turn-1",
            "status": "waiting",
        })
    );
}

#[test]
fn goal_resume_keeps_the_session_until_the_process_exits() {
    let resume = Cli::try_parse_from(["talos", "goal", "--session", "s1", "resume"]).unwrap();
    let Some(Commands::Goal(cmd)) = resume.command else {
        panic!("goal resume");
    };
    let policy = session_ownership(&TerminalMode::Command(Commands::Goal(cmd)));
    assert_eq!(policy.hold, talos_base::SessionHold::Process);

    let show = Cli::try_parse_from(["talos", "goal", "--session", "s1", "show"]).unwrap();
    let Some(Commands::Goal(cmd)) = show.command else {
        panic!("goal show");
    };
    let policy = session_ownership(&TerminalMode::Command(Commands::Goal(cmd)));
    assert_eq!(policy.hold, talos_base::SessionHold::Turn);
}

#[test]
fn parses_exec_request_id() {
    let cli = Cli::try_parse_from(["talos", "exec", "--request-id", "key-1", "hello"]).unwrap();
    assert!(matches!(
        cli.command,
        Some(Commands::Exec(cmd)) if cmd.request_id.as_deref() == Some("key-1")
    ));
}

#[test]
fn parses_exec_command() {
    for (args, expected) in [
        (vec!["talos", "exec", "hello"], false),
        (
            vec!["talos", "exec", "--dangerously-skip-permissions", "hello"],
            true,
        ),
    ] {
        let cli = Cli::try_parse_from(args).unwrap();
        assert!(
            matches!(cli.command, Some(Commands::Exec(cmd)) if cmd.dangerously_skip_permissions == expected)
        );
    }
}

#[tokio::test]
async fn waiting_finishes_with_the_latest_usage_while_the_stream_is_still_open() {
    let (sender, receiver) = mpsc::channel(3);
    let first = TokenUsage {
        input_tokens: 10,
        total_tokens: 10,
        ..Default::default()
    };
    let last = TokenUsage {
        input_tokens: 20,
        output_tokens: 5,
        total_tokens: 25,
        cache_read_input_tokens: Some(3),
        ..Default::default()
    };
    for usage in [first, last.clone()] {
        sender
            .send(timeline_event(AgentEventPayload::MetricsUpdated(
                AgentRunMetrics {
                    usage,
                    ..Default::default()
                },
            )))
            .await
            .unwrap();
    }
    sender
        .send(timeline_event(AgentEventPayload::WaitingForInput(
            AgentPendingInput::Message {
                action_id: "action-1".into(),
                prompt: "Waiting".into(),
            },
        )))
        .await
        .unwrap();
    let mut observation = ExecObservation::default();
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        render_timeline_events(receiver, &mut observation),
    )
    .await
    .expect("waiting should stop before the sender is dropped");
    assert!(result.unwrap_err().is::<ExecWaitingForInput>());
    assert_eq!(observation.metrics.unwrap().usage, last);
}

#[tokio::test]
async fn exec_reports_a_stream_that_closes_without_a_terminal_event() {
    let (sender, receiver) = mpsc::channel(1);
    drop(sender);
    let mut observation = ExecObservation::default();
    assert!(render_timeline_events(receiver, &mut observation)
        .await
        .is_err());
    assert_eq!(observation.metrics, None);
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
    assert!(matches!(command, types::PluginCommands::Installation(
        types::PluginInstallationCommand::Install {
            plugin_id, commit: Some(commit), disabled: true, replace: true,
        }
    ) if plugin_id == "plugin-browser" && commit == "fixed-revision"));
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
        use types::PluginInstallationCommand::{Disable, Enable, Uninstall, Update};
        let install_id = match command {
            types::PluginCommands::Installation(
                Enable { install_id }
                | Disable { install_id }
                | Update { install_id, .. }
                | Uninstall { install_id },
            ) => install_id,
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

#[test]
fn goal_and_loop_cli_require_an_existing_session_and_preserve_named_actions() {
    assert!(Cli::try_parse_from(["talos", "goal", "start", "objective"]).is_err());
    assert!(Cli::try_parse_from(["talos", "loop", "list"]).is_err());
    let goal = Cli::try_parse_from([
        "talos",
        "goal",
        "--session",
        "s",
        "clear",
        "--id",
        "old-goal",
    ])
    .unwrap();
    assert!(
        matches!(goal.command,Some(Commands::Goal(types::GoalCmd {session,command:Some(types::GoalCommands::Clear {id:Some(id)})})) if session=="s" && id=="old-goal")
    );
    let loop_cmd = Cli::try_parse_from([
        "talos",
        "loop",
        "--session",
        "s",
        "edit",
        "loop-1",
        "--every",
        "5",
        "new prompt",
    ])
    .unwrap();
    assert!(
        matches!(loop_cmd.command,Some(Commands::Loop(types::LoopCmd {session,command:Some(types::LoopCommands::Edit {id,every:Some(5),dynamic:false,prompt})})) if session=="s" && id=="loop-1" && prompt==vec!["new prompt"])
    );
    assert!(Cli::try_parse_from([
        "talos",
        "loop",
        "--session",
        "s",
        "start",
        "--every",
        "5",
        "--dynamic",
        "prompt"
    ])
    .is_err());
}
#[test]
fn cli_flags_build_typed_schedules_without_parsing_or_rewriting_prompt() {
    use talos_sdk::{LoopAction, LoopSchedule};
    let prompt = "5m 每 10 分钟检查 CI";
    assert_eq!(
        automation::loop_create(prompt.into(), Some(5), false).unwrap(),
        LoopAction::Create {
            prompt: prompt.into(),
            schedule: LoopSchedule::FixedInterval { minutes: 5 }
        }
    );
    assert_eq!(
        automation::loop_create(prompt.into(), None, true).unwrap(),
        LoopAction::Create {
            prompt: prompt.into(),
            schedule: LoopSchedule::ModelDecided {}
        }
    );
    assert_eq!(
        automation::loop_schedule(None, false).unwrap(),
        LoopSchedule::ModelDecided {}
    );
    assert!(automation::loop_schedule(Some(0), false).is_err());
    assert!(automation::loop_schedule(Some(5), true).is_err());
}
#[test]
fn native_jobs_cli_requires_computer_and_keeps_owner_scoped_action() {
    assert!(Cli::try_parse_from(["talos", "automation", "jobs", "list"]).is_err());
    let command = Cli::try_parse_from([
        "talos",
        "automation",
        "jobs",
        "--computer",
        "computer-1",
        "run-once",
        "profile-usage-retention",
    ])
    .unwrap();
    assert!(
        matches!(command.command,Some(Commands::Automation(types::AutomationCmd {command:types::AutomationCommands::Jobs {computer,command:Some(types::JobCommands::RunOnce {id})}})) if computer=="computer-1" && id=="profile-usage-retention")
    );
}

#[test]
fn foreground_creation_uses_owner_receipt_even_when_snapshot_is_already_empty() {
    use talos_sdk::{
        AutomationControlResult, AutomationEffect, AutomationObjectRef, SessionAutomationSnapshot,
    };
    let mut result = AutomationControlResult {
        snapshot: SessionAutomationSnapshot {
            session_id: "s".into(),
            goal: None,
            loops: vec![],
        },
        effect: AutomationEffect::Applied,
        bound_origin: None,
        created_object: Some(AutomationObjectRef::Loop {
            loop_id: "exact-created-loop".into(),
        }),
    };
    assert_eq!(
        automation::created_target(&result).unwrap(),
        AutomationObjectRef::Loop {
            loop_id: "exact-created-loop".into()
        }
    );
    result.created_object = None;
    assert!(automation::created_target(&result).is_err());
}
