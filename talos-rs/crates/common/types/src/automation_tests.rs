use super::*;
use serde_json::json;
use ts_rs::TS;

#[test]
fn automation_wire_preserves_revision_and_uses_closed_actions() {
    let action = AutomationAction::Loop {
        action: LoopAction::Decide {
            loop_id: "loop-1".into(),
            config_revision: 7,
            decision: LoopWakeDecision::Continue {
                delay_minutes: 2,
                reason: "Deployment is progressing".into(),
            },
        },
    };
    let expected = json!({
        "target": "loop",
        "action": {
            "type": "decide", "loopId": "loop-1", "configRevision": 7,
            "decision": {"type": "continue", "delayMinutes": 2, "reason": "Deployment is progressing"}
        }
    });
    assert_eq!(serde_json::to_value(&action).unwrap(), expected);
    assert_eq!(
        serde_json::from_value::<AutomationAction>(expected).unwrap(),
        action
    );
    assert!(serde_json::from_value::<AutomationAction>(json!({
        "target": "builtinJob", "action": {"type": "runOnce"}
    }))
    .is_err());
    assert!(serde_json::from_value::<BuiltinJobId>(json!("loop-scan")).is_err());
}

#[test]
fn interval_zero_is_rejected_and_model_schedule_needs_no_fake_interval() {
    assert!(LoopSchedule::FixedInterval { minutes: 0 }
        .validate()
        .is_err());
    assert!(LoopSchedule::FixedInterval { minutes: 1 }
        .validate()
        .is_ok());
    assert!(LoopSchedule::ModelDecided {}.validate().is_ok());
    assert_eq!(
        serde_json::to_value(LoopSchedule::ModelDecided {}).unwrap(),
        json!({"type": "modelDecided"})
    );
}

#[test]
fn nullable_next_time_and_original_goal_budget_are_explicit() {
    let goal = SessionGoal {
        id: "goal-1".into(),
        session_id: "session-1".into(),
        objective: "Pass the tests".into(),
        objective_revision: 2,
        status: GoalStatus::Paused,
        pause_reason: Some(GoalPauseReason::UserPaused),
        reason: None,
        token_budget: Some(1000),
        tokens_used: 150,
        latest_turn_id: Some("turn-1".into()),
        completion_evidence: None,
        created_at: 1,
        updated_at: 2,
    };
    let snapshot = SessionAutomationSnapshot {
        session_id: "session-1".into(),
        goal: Some(goal),
        loops: vec![],
    };
    let encoded = serde_json::to_value(&snapshot).unwrap();
    assert_eq!(encoded["goal"]["tokenBudget"], 1000);
    assert_eq!(
        encoded["goal"]["completionEvidence"],
        serde_json::Value::Null
    );
    assert_eq!(
        serde_json::from_value::<SessionAutomationSnapshot>(encoded).unwrap(),
        snapshot
    );
    assert_eq!(
        serde_json::to_value(QueuedMessageSource::default()).unwrap(),
        json!({"type": "user"})
    );
}

#[test]
fn tool_acknowledges_pending_declarations_without_faking_completion() {
    let result = AutomationControlResult {
        created_object: None,
        effect: AutomationEffect::PendingSettlement,
        bound_origin: None,
        snapshot: SessionAutomationSnapshot {
            session_id: "s".into(),
            goal: None,
            loops: vec![],
        },
    };
    assert_eq!(
        serde_json::to_value(result).unwrap()["effect"],
        "pendingSettlement"
    );
}

#[test]
fn typescript_and_wire_agree_on_action_field_names() {
    let declaration = LoopAction::decl();
    assert!(declaration.contains("loopId"), "{declaration}");
    assert!(declaration.contains("expectedRevision"), "{declaration}");
    assert!(!declaration.contains("loop_id"), "{declaration}");
    let declaration = LoopWakeDecision::decl();
    assert!(declaration.contains("delayMinutes"), "{declaration}");
}

#[test]
fn automation_typescript_preserves_wire_discriminants() {
    for (declaration, tag, variants) in [
        (AutomationAction::decl(), "target", vec!["goal", "loop"]),
        (
            GoalAction::decl(),
            "type",
            vec![
                "create", "read", "edit", "pause", "resume", "clear", "complete",
            ],
        ),
        (
            LoopAction::decl(),
            "type",
            vec!["create", "list", "edit", "stop", "clear", "decide"],
        ),
        (LoopWakeDecision::decl(), "type", vec!["continue", "stop"]),
        (
            LoopSchedule::decl(),
            "type",
            vec!["fixedInterval", "modelDecided"],
        ),
    ] {
        for variant in variants {
            let discriminant = format!("\"{tag}\": \"{variant}\"");
            assert!(declaration.contains(&discriminant), "{declaration}");
        }
    }
}

#[test]
fn only_affirmative_original_requests_enable_automation() {
    for (text, expected) in [
        (
            "使用 Goal 持续修复，直到测试通过",
            Some(AutomationMode::Goal),
        ),
        ("用 Loop 每 5 分钟检查 CI", Some(AutomationMode::Loop)),
        ("please use loop to check CI", Some(AutomationMode::Loop)),
        ("帮我用goal完成迁移", Some(AutomationMode::Goal)),
        ("请使用 Goal 检查 CI，可以吗？", Some(AutomationMode::Goal)),
        ("请帮我使用 Goal 修复 CI", Some(AutomationMode::Goal)),
        (
            "我希望用 Loop 每 5 分钟检查 CI？",
            Some(AutomationMode::Loop),
        ),
        (
            "Could you please use Goal to fix CI?",
            Some(AutomationMode::Goal),
        ),
        (
            "使用模型决定间隔的 Loop 检查 CI",
            Some(AutomationMode::Loop),
        ),
        ("使用 Goal 是什么意思？", None),
        ("使用 Goal 支持哪些工具？", None),
        ("用 Goal 会执行哪些操作？", None),
        ("用 Goal 怎么配置？", None),
        ("使用 Goal 和 Loop 有什么区别？", None),
        ("请使用 Goal，可以吗？", None),
        ("goal 和 loop 有什么区别？", None),
        ("不要使用 Goal 修复", None),
        ("what is loop", None),
        ("> 使用 Goal 修复", None),
        ("```\n使用 Goal 修复\n```", None),
        ("“使用 Goal 修复”", None),
        ("use looper to check CI", None),
        ("repair tests until they pass", None),
        ("/goal repair tests", Some(AutomationMode::Goal)),
        ("/loop 5m check CI", Some(AutomationMode::Loop)),
        ("/goal", None),
        ("/loop list", None),
        ("/loop stop loop-1", None),
    ] {
        let input = AgentUserInput {
            content: vec![AgentUserContent::Text { text: text.into() }],
        };
        assert_eq!(
            explicit_automation_mode(&input).unwrap(),
            expected,
            "{text}"
        );
    }
    let input = AgentUserInput {
        content: vec![
            AgentUserContent::SkillSelection {
                name: "goal".into(),
                path: "/system/goal/SKILL.md".into(),
            },
            AgentUserContent::SkillSelection {
                name: "loop".into(),
                path: "/system/loop/SKILL.md".into(),
            },
        ],
    };
    assert!(explicit_automation_mode(&input).is_err());
}

#[test]
fn shared_commands_preserve_prompt_and_do_not_create_from_management_words() {
    assert_eq!(
        resolve_automation_command("/loop 5m check CI and report changes").unwrap(),
        Some(AutomationCommand::Activate {
            mode: AutomationMode::Loop,
            prompt: "5m check CI and report changes".into(),
        })
    );
    assert_eq!(
        resolve_automation_command("/goal").unwrap(),
        Some(AutomationCommand::Read {
            mode: AutomationMode::Goal
        })
    );
    assert_eq!(
        resolve_automation_command("/loop stop loop-1").unwrap(),
        Some(AutomationCommand::StopLoop {
            loop_id: "loop-1".into()
        })
    );
    for input in ["/loop stop", "/loop stop a b", "/goal pause extra"] {
        assert!(resolve_automation_command(input).is_err(), "{input}");
    }
    for prompt in [
        "0m check",
        "1m",
        "every 30 seconds check CI",
        "每两小时检查一次",
        "wait until deployment changes",
    ] {
        assert_eq!(
            resolve_automation_command(&format!("/loop {prompt}")).unwrap(),
            Some(AutomationCommand::Activate {
                mode: AutomationMode::Loop,
                prompt: prompt.into(),
            })
        );
    }
    assert_eq!(resolve_automation_command("/other command").unwrap(), None);
    assert_eq!(automation_command_catalog().len(), 2);
}

#[test]
fn automation_schema_uses_the_same_camel_case_variant_fields_as_json() {
    fn assert_camel_case(schema: &serde_json::Value) {
        match schema {
            serde_json::Value::Object(object) => {
                if let Some(serde_json::Value::Object(properties)) = object.get("properties") {
                    for field in properties.keys() {
                        assert!(!field.contains('_'), "non-wire schema field: {field}");
                    }
                }
                for child in object.values() {
                    assert_camel_case(child);
                }
            }
            serde_json::Value::Array(values) => {
                for child in values {
                    assert_camel_case(child);
                }
            }
            _ => {}
        }
    }
    macro_rules! check {
        ($($ty:ty),+ $(,)?) => {
            $(assert_camel_case(&serde_json::to_value(schemars::schema_for!($ty)).unwrap());)+
        };
    }
    check!(
        LoopSchedule,
        AutomationAction,
        GoalAction,
        LoopAction,
        LoopWakeDecision,
        AutomationObjectRef,
        AutomationTurnOrigin,
        QueuedMessageSource,
        AutomationListEntry,
        AutomationCommand,
    );
}

#[test]
fn creation_reference_is_nullable_in_wire_and_typescript() {
    let result = AutomationControlResult {
        effect: AutomationEffect::Applied,
        created_object: None,
        bound_origin: None,
        snapshot: SessionAutomationSnapshot {
            session_id: "s".into(),
            goal: None,
            loops: vec![],
        },
    };
    let encoded = serde_json::to_value(result).unwrap();
    assert_eq!(encoded.get("createdObject"), Some(&serde_json::Value::Null));
    let declaration = AutomationControlResult::decl();
    assert!(declaration.contains("createdObject:"), "{declaration}");
    assert!(!declaration.contains("createdObject?:"), "{declaration}");
}

#[test]
fn automation_unit_operations_reject_unknown_fields() {
    let mut accepted = Vec::new();
    for input in [
        json!({"target":"goal","action":{"type":"read","extra":true}}),
        json!({"target":"loop","action":{"type":"list","extra":true}}),
        json!({"target":"loop","action":{"type":"create","prompt":"Check CI","schedule":{"type":"modelDecided","minutes":5}}}),
    ] {
        if serde_json::from_value::<AutomationAction>(input.clone()).is_ok() {
            accepted.push(input);
        }
    }
    assert!(
        accepted.is_empty(),
        "Unknown fields were accepted: {accepted:?}"
    );
}
