use super::*;

#[tokio::test]
async fn test_prompt_builder_basic() {
    let assembly = PromptBuilder::new()
        .agent("talos", "assistant")
        .phase(PromptPhase::Execution)
        .user_input("hello")
        .assemble()
        .await
        .expect("prompt build should succeed");

    assert!(assembly
        .static_prefix_sections
        .iter()
        .any(|section| section.kind == PromptSectionKind::IdentityCore));
    assert_eq!(assembly.user_input, "hello");
    assert!(assembly
        .request_context_injection
        .prepended_user_context_message
        .as_ref()
        .expect("time reminder should exist")
        .contains("<current-date>"));

    let messages = render_prompt_assembly(&assembly);
    assert_eq!(
        messages.last().expect("user message should exist").role,
        Role::User
    );
    assert_eq!(
        messages.last().expect("user message should exist").content,
        "hello"
    );
}

#[tokio::test]
async fn test_prompt_blocks_follow_expected_order() {
    let registry = ToolRegistry::new();
    registry.register(Arc::new(EchoTool));

    let assembly = PromptBuilder::new()
        .phase(PromptPhase::Planning)
        .planner_mode("plan_react")
        .expect("planner mode should parse")
        .planning_instruction(Some("build a short plan first".to_string()))
        .tools(&registry)
        .user_input("hello")
        .assemble()
        .await
        .expect("prompt assembly should succeed");

    let blocks = render_prompt_blocks(&assembly);
    let kinds = blocks.iter().map(|block| block.kind).collect::<Vec<_>>();
    assert!(matches!(kinds.first(), Some(PromptBlockKind::StaticPrefix)));
    assert!(kinds.contains(&PromptBlockKind::DynamicSuffix));
    assert!(kinds.contains(&PromptBlockKind::AppendedSystemContext));
    assert!(kinds.contains(&PromptBlockKind::PrependedUserContextMessage));
    assert!(matches!(kinds.last(), Some(PromptBlockKind::UserInput)));
}

#[tokio::test]
async fn test_prompt_builder_includes_tool_summary_and_planning_instruction() {
    let registry = ToolRegistry::new();
    registry.register(Arc::new(EchoTool));

    let blocks = render_prompt_blocks(
        &PromptBuilder::new()
            .phase(PromptPhase::Planning)
            .planner_mode("plan_react")
            .expect("planner mode should parse")
            .planning_instruction(Some("build a short plan first".to_string()))
            .tools(&registry)
            .user_input("help me")
            .assemble()
            .await
            .expect("prompt build should succeed"),
    );

    let dynamic = blocks
        .iter()
        .find(|block| block.kind == PromptBlockKind::DynamicSuffix)
        .expect("dynamic block should exist");
    assert!(dynamic.text.contains("build a short plan first"));
    assert!(dynamic.text.contains("echo"));
}

#[tokio::test]
async fn test_prompt_builder_keeps_history_without_legacy_budget_caps() {
    let memory = MemoryStore::new(std::env::temp_dir().join(Uuid::new_v4().to_string()));
    for idx in 0..6 {
        let turn_id = format!("turn-{idx}");
        let timestamp = Utc::now();
        memory
            .append_turn(&TalosTurn {
                turn_id: turn_id.clone(),
                message_id: "msg-1".to_string(),
                session_id: "s1".to_string(),
                seq: memory.next_turn_seq("s1").await.expect("seq should load"),
                timestamp,
                items: vec![
                    TalosOutputItem::UserInput {
                        text: format!("historical user {}", idx),
                    },
                    TalosOutputItem::OutputText {
                        text: format!("historical message {}", idx),
                    },
                ],
                usage: TalosUsage::default(),
                stop_reason: StopReason::EndTurn,
            })
            .await
            .expect("append should succeed");
    }

    let assembly = PromptBuilder::new()
        .memory_store("s1", &memory)
        .user_input("latest")
        .assemble()
        .await
        .expect("prompt build should succeed");

    assert!(assembly
        .dynamic_suffix_sections
        .iter()
        .any(|section| section.kind == PromptSectionKind::RecentMessages));
    assert_eq!(
        assembly
            .dynamic_suffix_sections
            .iter()
            .filter(|section| section.kind == PromptSectionKind::RecentMessages)
            .count(),
        1
    );
    assert!(assembly.diagnostics.dropped_sections.is_empty());
}

#[test]
fn test_prompt_window_usage_derives_default_reserved_output_tokens() {
    let window_usage =
        PromptWindowUsage::resolve(&PromptBuildConfig::default().prompt, 32_768, 1_024, 2_048);

    assert!(window_usage.window_resolved);
    assert_eq!(window_usage.context_window, 32_768);
    assert_eq!(window_usage.reserved_output_tokens, 4_096);
    assert_eq!(window_usage.compaction_buffer_tokens, 4_096);
    assert_eq!(window_usage.effective_prompt_budget, 24_576);
}

#[test]
fn test_prompt_window_usage_clamps_derived_reserved_output_tokens() {
    let window_usage =
        PromptWindowUsage::resolve(&PromptBuildConfig::default().prompt, 16_384, 1_024, 2_048);

    assert_eq!(window_usage.reserved_output_tokens, 2_048);
    assert_eq!(window_usage.compaction_buffer_tokens, 4_096);
    assert_eq!(window_usage.effective_prompt_budget, 10_240);
}

#[test]
fn test_prompt_window_usage_has_no_default_reserved_output_when_context_window_is_unresolved() {
    let window_usage =
        PromptWindowUsage::resolve(&PromptBuildConfig::default().prompt, 0, 1_024, 2_048);

    assert!(!window_usage.window_resolved);
    assert_eq!(window_usage.reserved_output_tokens, 0);
    assert_eq!(window_usage.compaction_buffer_tokens, 4_096);
    assert_eq!(window_usage.effective_prompt_budget, 0);
    assert!(window_usage.within_effective_budget);
}

#[test]
fn test_prompt_window_usage_clamps_fixed_policy_and_ratio_branches_independently() {
    let mut fixed_config = PromptBuildConfig::default();
    fixed_config.prompt.window.reserved_output_tokens =
        talos_config::ReservedOutputPolicy::Fixed { tokens: 512 };
    let fixed_usage = PromptWindowUsage::resolve(&fixed_config.prompt, 4_096, 256, 256);
    assert_eq!(fixed_usage.reserved_output_tokens, 512);
    assert_eq!(fixed_usage.compaction_buffer_tokens, 1_024);
    assert_eq!(fixed_usage.effective_prompt_budget, 2_560);

    let mut ratio_config = PromptBuildConfig::default();
    ratio_config.prompt.window.reserved_output_tokens = talos_config::ReservedOutputPolicy::Ratio {
        numerator: 1,
        denominator: 4,
    };
    let ratio_usage = PromptWindowUsage::resolve(&ratio_config.prompt, 8_192, 256, 256);
    assert_eq!(ratio_usage.reserved_output_tokens, 2_048);
    assert_eq!(ratio_usage.compaction_buffer_tokens, 2_048);
    assert_eq!(ratio_usage.effective_prompt_budget, 4_096);
}

#[tokio::test]
async fn test_prompt_builder_drops_expanded_tool_catalog_before_higher_priority_dynamic_regions() {
    let mut config = PromptBuildConfig::default();
    config.prompt.window.reserved_output_tokens =
        talos_config::ReservedOutputPolicy::Fixed { tokens: 256 };
    config.prompt.window.compaction_buffer_tokens = 128;
    config.load_policy.include_session_summary = false;
    config.load_policy.include_recent_messages = false;

    let baseline = PromptBuilder::new()
        .config(config.clone())
        .model_info(ModelInfo {
            id: "baseline".to_string(),
            name: "baseline".to_string(),
            provider: "test".to_string(),
            context_window: 16_384,
            reasoning: false,
            input: Vec::new(),
            cost: Default::default(),
            compat: Default::default(),
        })
        .planning_instruction(Some("keep planning".to_string()))
        .user_input("hello")
        .assemble()
        .await
        .expect("baseline prompt should build");

    let effective_budget = baseline.diagnostics.total_estimated_tokens + 32;
    let registry = ToolRegistry::new();
    for index in 0..6 {
        registry.register(Arc::new(VerboseTool {
            name: match index {
                0 => "alpha_tool",
                1 => "beta_tool",
                2 => "gamma_tool",
                3 => "delta_tool",
                4 => "epsilon_tool",
                _ => "zeta_tool",
            },
            description: "Inspect repository structure, ownership boundaries, runtime behavior, test surfaces, deployment assumptions, and implementation drift across subsystems with detailed evidence requirements.",
        }));
    }

    let assembly = PromptBuilder::new()
        .tools(&registry)
        .config(config)
        .planning_instruction(Some("keep planning".to_string()))
        .model_info(ModelInfo {
            id: "tight-window".to_string(),
            name: "tight-window".to_string(),
            provider: "test".to_string(),
            context_window: (effective_budget + 256 + 128) as u32,
            reasoning: false,
            input: Vec::new(),
            cost: Default::default(),
            compat: Default::default(),
        })
        .user_input("hello")
        .assemble()
        .await
        .expect("prompt should build");

    assert!(assembly.diagnostics.window_usage.within_effective_budget);
    assert!(assembly
        .diagnostics
        .dropped_sections
        .contains(&PromptSectionKind::ExpandedToolCatalog));
    assert!(assembly
        .dynamic_suffix_sections
        .iter()
        .all(|section| section.kind != PromptSectionKind::ExpandedToolCatalog));
    assert!(assembly
        .dynamic_suffix_sections
        .iter()
        .any(|section| section.kind == PromptSectionKind::PlanningInstruction));
}
