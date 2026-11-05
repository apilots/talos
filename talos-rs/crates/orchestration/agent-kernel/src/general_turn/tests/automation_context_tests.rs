//! Capture actual model requests from the normal Turn preparation/execution path.
use super::attachment_tests::RecordingAttachmentProvider;
use super::*;
use talos_tools::{AutomationControl, AutomationControlFuture};
use talos_types::{
    AutomationControlResult, AutomationEffect, AutomationMode, AutomationModelRequest,
    AutomationTurnOrigin, GoalStatus, SessionAutomationSnapshot, SessionGoal, SkillScope,
};

struct SnapshotControl {
    result: AutomationControlResult,
    calls: Mutex<Vec<AutomationModelRequest>>,
}
impl AutomationControl for SnapshotControl {
    fn execute(&self, request: AutomationModelRequest) -> AutomationControlFuture<'_> {
        self.calls.lock().unwrap().push(request);
        Box::pin(async { Ok(self.result.clone()) })
    }
}
fn control(origin: Option<AutomationTurnOrigin>) -> Arc<SnapshotControl> {
    let goal = matches!(origin, Some(AutomationTurnOrigin::Goal { .. })).then(|| SessionGoal {
        id: "g".into(),
        session_id: "s1".into(),
        objective: "Finish tests".into(),
        objective_revision: 1,
        status: GoalStatus::Active,
        pause_reason: None,
        reason: None,
        token_budget: None,
        tokens_used: 0,
        latest_turn_id: Some("i1".into()),
        completion_evidence: None,
        created_at: 1,
        updated_at: 1,
    });
    Arc::new(SnapshotControl {
        calls: Mutex::new(vec![]),
        result: AutomationControlResult {
            created_object: None,
            effect: AutomationEffect::Applied,
            bound_origin: origin,
            snapshot: SessionAutomationSnapshot {
                session_id: "s1".into(),
                goal,
                loops: vec![],
            },
        },
    })
}
fn system_roots(workspace: &Path) -> SkillRoots {
    let system = workspace.join("system-skills");
    for skill in talos_skills::builtin_automation_skills() {
        let folder = system.join(skill.mode.skill_name());
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("SKILL.md"), skill.contents).unwrap();
    }
    SkillRoots {
        roots: vec![SkillRoot {
            path: system,
            scope: SkillScope::System,
            source_plugin: None,
            source_install: None,
            read_only: true,
        }],
        state_path: workspace.join("skills-state.json"),
        max_skill_md_bytes: 65536,
        max_available_summary_bytes: 8192,
    }
}

#[tokio::test]
async fn original_activation_is_explicit_in_actual_model_request_without_rewriting_input() {
    let workspace = std::env::temp_dir().join(Uuid::new_v4().to_string());
    let roots = system_roots(&workspace);
    let goal_origin = AutomationTurnOrigin::Goal {
        goal_id: "g".into(),
        objective_revision: 1,
    };
    let loop_origin = AutomationTurnOrigin::Loop {
        loop_id: "l".into(),
        occurrence_id: "o".into(),
        config_revision: 1,
    };
    for (mode, selected, origin, input, expected_request) in [
        (
            AutomationMode::Goal,
            true,
            None,
            "Fix these tests until all pass.",
            Some("explicit_activation"),
        ),
        (
            AutomationMode::Goal,
            false,
            None,
            "使用 Goal 持续修复这些测试，直到全部通过。",
            None,
        ),
        (
            AutomationMode::Goal,
            true,
            Some(goal_origin),
            "Fix these tests until all pass.",
            Some("bound_automatic_work"),
        ),
        (
            AutomationMode::Loop,
            true,
            Some(loop_origin),
            "Check CI status.",
            Some("bound_automatic_work"),
        ),
        (
            AutomationMode::Goal,
            false,
            None,
            "Fix these tests until all pass.",
            None,
        ),
    ] {
        let control = control(origin);
        let registry = Arc::new(ToolRegistry::new());
        registry.set_automation_control(control.clone());
        let provider = Arc::new(RecordingAttachmentProvider::default());
        let mut config = AgentKernelConfig::default();
        config.prompt.bootstrap.enabled = false;
        config.workspace_dir = workspace.to_string_lossy().into_owned();
        let mut ctx = build_ctx_with_stream(None);
        ctx.input.text = input.into();
        let options = GeneralTurnSkillOptions {
            selected: if selected {
                vec![talos_skills::system_automation_selector(&roots, mode).unwrap()]
            } else {
                vec![]
            },
            roots: Some(roots.clone()),
        };
        run_general_turn(
            &mut ctx,
            "talos",
            "assistant",
            input,
            provider.clone(),
            registry.clone(),
            registry,
            Arc::new(TestMemoryStore::new()),
            &config,
            options,
        )
        .await
        .unwrap();
        let requests = provider.requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].messages.last().unwrap().content, input);
        assert_eq!(ctx.input.text, input);
        let context = requests[0].messages.iter().find(|message| {
            message
                .content
                .contains("[AutomationInvocation]\nBackend-confirmed facts")
        });
        if let Some(expected) = expected_request {
            let context =
                context.expect("backend invocation facts must reach the actual model request");
            assert!(context
                .content
                .contains(&format!("current_request: {expected}")));
            assert!(context
                .content
                .contains(&format!("selected_capability: {}", mode.skill_name())));
            assert!(context.content.contains(&format!(
                "activation_requested: {}",
                expected == "explicit_activation"
            )));
            assert_eq!(
                context.input_origin.as_ref().unwrap().source,
                talos_llm_provider::MessageInputSource::Runtime
            );
            let calls = control.calls.lock().unwrap();
            assert_eq!(calls.len(), 1);
            assert_eq!(calls[0].caller.turn_id, ctx.invocation_id);
        } else {
            assert!(context.is_none());
            assert!(control.calls.lock().unwrap().is_empty());
            let instructions = requests[0]
                .messages
                .iter()
                .map(|message| message.content.as_str())
                .collect::<Vec<_>>()
                .join("\n");
            assert!(instructions.contains("Available skills:"));
            for skill in talos_skills::builtin_automation_skills() {
                assert!(instructions.contains(&format!("{} [system]", skill.mode.skill_name())));
                assert!(!instructions.contains(skill.contents));
            }
        }
    }
    std::fs::remove_dir_all(workspace).unwrap();
}

#[tokio::test]
async fn activation_facts_survive_real_history_compaction_and_prompt_rebuild() {
    let workspace = std::env::temp_dir().join(Uuid::new_v4().to_string());
    let roots = system_roots(&workspace);
    let provider = Arc::new(SummaryProvider::new(vec![
        r#"{"mode":"compaction","title":"Prompt Compact","facts":["history condensed"],"open_tasks":["continue the current turn"]}"#.into(),
        "ok".into(),
    ]));
    let registry = Arc::new(ToolRegistry::new());
    registry.set_automation_control(control(None));
    let memory = Arc::new(TestMemoryStore::new());
    for index in 0..24 {
        memory.append_prompt_turn(
            format!("old-{index}"),
            i64::from(index + 1),
            vec![
                (
                    SessionPromptMessageRole::User,
                    format!("historical user message {index}"),
                ),
                (
                    SessionPromptMessageRole::Assistant,
                    "historical assistant evidence retained for the next user task. ".repeat(8),
                ),
            ],
        );
    }
    let mut config = AgentKernelConfig {
        workspace_dir: workspace.to_string_lossy().into_owned(),
        ..Default::default()
    };
    config.prompt.bootstrap.enabled = false;
    config.prompt.window.compaction_buffer_tokens = 32;
    config.prompt.window.reserved_output_tokens =
        talos_config::ReservedOutputPolicy::Fixed { tokens: 128 };
    config.compaction.recent_messages_keep = 24;
    let mut ctx = build_ctx_with_stream(None);
    ctx.selected_model_info.model.context_window = 4000;
    let input = "Fix these tests until all pass.";
    let options = GeneralTurnSkillOptions {
        selected: vec![
            talos_skills::system_automation_selector(&roots, AutomationMode::Goal).unwrap(),
        ],
        roots: Some(roots),
    };
    let result = run_general_turn(
        &mut ctx,
        "talos",
        "assistant",
        input,
        provider.clone(),
        registry.clone(),
        registry,
        memory,
        &config,
        options,
    )
    .await
    .unwrap();
    assert_eq!(result.output_text(), "ok");
    assert!(ctx.session.state.compaction.as_ref().unwrap().is_recovery());
    let requests = provider.requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    let model_request = requests.last().unwrap();
    assert_eq!(model_request.messages.last().unwrap().content, input);
    let facts = model_request
        .messages
        .iter()
        .filter(|message| {
            message
                .content
                .contains("[AutomationInvocation]\nBackend-confirmed facts")
        })
        .collect::<Vec<_>>();
    assert_eq!(facts.len(), 1);
    assert!(facts[0]
        .content
        .contains("current_request: explicit_activation"));
    std::fs::remove_dir_all(workspace).unwrap();
}
