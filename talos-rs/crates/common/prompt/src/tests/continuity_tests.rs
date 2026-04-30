use super::*;

#[tokio::test]

async fn test_prompt_builder_includes_recent_task_continuity_section_with_xml_envelope() {
    let assembly = PromptBuilder::new()
        .session_continuity(Some(SessionContinuity {
            recent_goal: "inspect the prompt builder continuity output".to_string(),
            verified_paths: vec![VerifiedPathEvidence {
                path: "/tmp/project/Cargo.toml".to_string(),
                source_tool: "Read".to_string(),
            }],
            tool_summaries: vec![
                ContinuityToolSummary {
                    tool_name: "Grep".to_string(),
                    outcome: ContinuityToolOutcome::Success,
                },
                ContinuityToolSummary {
                    tool_name: "Read".to_string(),
                    outcome: ContinuityToolOutcome::Success,
                },
                ContinuityToolSummary {
                    tool_name: "Bash".to_string(),
                    outcome: ContinuityToolOutcome::Failure,
                },
            ],
            pending_tasks: vec!["complete the answer from the verified evidence".to_string()],
            recent_unhelpful_patterns: vec![
                "avoid retrying the same failing path or command".to_string()
            ],
            recent_answer_mode: Some(ContinuityAnswerMode::CodeFirst),
            last_failure: Some("Bash returned an error".to_string()),
            suggested_next_step: Some(
                "answer from the verified evidence without re-reading the same paths".to_string(),
            ),
        }))
        .user_input("hello")
        .assemble()
        .await
        .expect("prompt assembly should succeed");

    let dynamic_block = render_prompt_blocks(&assembly)
        .into_iter()
        .find(|block| block.kind == PromptBlockKind::DynamicSuffix)
        .expect("dynamic block should exist");
    assert!(dynamic_block
        .text
        .contains("<region-name>recent_task_continuity</region-name>"));
    assert!(dynamic_block
        .text
        .contains("- recent_goal: inspect the prompt builder continuity output"));
    assert!(dynamic_block
        .text
        .contains("- pending_tasks: complete the answer from the verified evidence"));
    assert!(dynamic_block.text.contains("code_first"));
    assert!(dynamic_block
        .text
        .contains("- last_failure: Bash returned an error"));
    assert!(dynamic_block.text.contains(
        "- suggested_next_step: answer from the verified evidence without re-reading the same paths"
    ));
    assert!(!dynamic_block.text.contains("<region-guidance>"));
}

#[test]
fn test_session_continuity_round_trips_with_pending_tasks() {
    let continuity = SessionContinuity {
        recent_goal: "inspect the staged continuity payload".to_string(),
        verified_paths: vec![VerifiedPathEvidence {
            path: "/tmp/project/Cargo.toml".to_string(),
            source_tool: "Read".to_string(),
        }],
        tool_summaries: vec![ContinuityToolSummary {
            tool_name: "Read".to_string(),
            outcome: ContinuityToolOutcome::Success,
        }],
        pending_tasks: vec!["complete the answer from the verified evidence".to_string()],
        recent_unhelpful_patterns: vec![
            "avoid re-reading the same path when no new evidence is needed".to_string(),
        ],
        recent_answer_mode: Some(ContinuityAnswerMode::CodeFirst),
        last_failure: None,
        suggested_next_step: Some(
            "answer from the verified evidence without re-reading the same paths".to_string(),
        ),
    };

    let value = serde_json::to_value(&continuity).expect("continuity should serialize");
    let decoded: SessionContinuity =
        serde_json::from_value(value).expect("continuity should deserialize");
    assert_eq!(decoded, continuity);
}
