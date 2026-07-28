//! Presentation-only projection for server-owned Subagent tool results.

use talos_sdk::{
    AgentTreeNode, CollaborationToolResult, SubagentOutcome, SubagentSnapshot, TalosContentItem,
};

use super::tool_projection::{
    completed_title, preview_lines_from_output, tool_title, ToolRenderData,
};

pub(super) fn project_subagent(
    name: &str,
    call_id: &str,
    arguments: &str,
    content: &[TalosContentItem],
    is_error: bool,
) -> Option<ToolRenderData> {
    let result = content.iter().find_map(|item| match item {
        TalosContentItem::SubagentResult { result } => Some(result),
        _ => None,
    })?;
    let mut lines = Vec::new();
    let summary = match result {
        CollaborationToolResult::SpawnAgent(spawned) => {
            lines.push(format!("agent: {}", spawned.subagent.path.as_str()));
            lines.push(format!("agent id: {}", spawned.agent_id));
            lines.push(format!("session: {}", spawned.subagent.session_id));
            lines.push(format!("turn: {}", spawned.turn.turn_id));
            format!("spawned {}", spawned.subagent.path.as_str())
        }
        CollaborationToolResult::SendMessage(delivery) => {
            lines.push(format!("agent: {}", delivery.subagent.path.as_str()));
            lines.push(format!("agent id: {}", delivery.agent_id));
            lines.push(format!("session: {}", delivery.subagent.session_id));
            lines.push(format!("turn: {}", delivery.turn.turn_id));
            format!("message accepted by {}", delivery.subagent.path.as_str())
        }
        CollaborationToolResult::WaitAgent(snapshot)
        | CollaborationToolResult::InterruptAgent(snapshot)
        | CollaborationToolResult::FollowupTask(snapshot) => {
            push_subagent_snapshot(&mut lines, snapshot);
            format!(
                "{} · {}",
                snapshot.subagent.path.as_str(),
                subagent_status_label(&snapshot.outcome)
            )
        }
        CollaborationToolResult::ListAgents(tree) => {
            for child in &tree.children {
                push_agent_tree_node(&mut lines, child, 0);
            }
            format!("{} agent(s)", lines.len())
        }
    };
    Some(ToolRenderData {
        title: completed_title(tool_title(name, call_id, arguments), is_error),
        summary,
        preview_lines: preview_lines_from_output(&lines, 2),
        output_lines: lines,
        force_show_output: false,
        ..ToolRenderData::default()
    })
}

fn push_subagent_snapshot(lines: &mut Vec<String>, snapshot: &SubagentSnapshot) {
    lines.push(format!("agent: {}", snapshot.agent_id));
    lines.push(format!("session: {}", snapshot.subagent.session_id));
    match &snapshot.outcome {
        SubagentOutcome::Completed {
            turn_id,
            output,
            truncated,
        } => {
            lines.push(format!("turn: {turn_id}"));
            if !output.trim().is_empty() {
                lines.push(if *truncated {
                    format!("{output} [truncated]")
                } else {
                    output.clone()
                });
            }
        }
        SubagentOutcome::Failed {
            turn_id,
            error,
            truncated,
        } => {
            lines.push(format!("turn: {turn_id}"));
            lines.push(if *truncated {
                format!("{error} [truncated]")
            } else {
                error.clone()
            });
        }
        SubagentOutcome::Running { turn_id }
        | SubagentOutcome::WaitingUserAction { turn_id }
        | SubagentOutcome::Cancelling { turn_id }
        | SubagentOutcome::Interrupted { turn_id } => {
            lines.push(format!("turn: {turn_id}"));
        }
    }
    if let Some(update) = &snapshot.latest_update {
        lines.push(if update.truncated {
            format!("{} [truncated]", update.text)
        } else {
            update.text.clone()
        });
    }
}

fn push_agent_tree_node(lines: &mut Vec<String>, node: &AgentTreeNode, depth: usize) {
    lines.push(format!(
        "{}{} · {}",
        "  ".repeat(depth),
        node.snapshot.subagent.path.as_str(),
        subagent_status_label(&node.snapshot.outcome)
    ));
    for child in &node.children {
        push_agent_tree_node(lines, child, depth + 1);
    }
}

fn subagent_status_label(outcome: &SubagentOutcome) -> &'static str {
    match outcome {
        SubagentOutcome::Running { .. } => "running",
        SubagentOutcome::WaitingUserAction { .. } => "waiting for user action",
        SubagentOutcome::Cancelling { .. } => "cancelling",
        SubagentOutcome::Completed { .. } => "completed",
        SubagentOutcome::Failed { .. } => "failed",
        SubagentOutcome::Interrupted { .. } => "interrupted",
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use talos_sdk::{
        AgentPath, AgentTurnRef, CollaborationToolResult, SendMessageResult, SubagentOutcome,
        SubagentRef, SubagentSnapshot, TalosContentItem,
    };

    use super::{project_subagent, subagent_status_label};

    #[test]
    fn projects_server_owned_subagent_result() {
        let projection = project_subagent(
            "wait_agent",
            "call-subagent",
            r#"{"target":{"type":"directChild","name":"review"}}"#,
            &[TalosContentItem::SubagentResult {
                result: CollaborationToolResult::WaitAgent(SubagentSnapshot {
                    subagent: SubagentRef {
                        path: AgentPath::from_str("/root/review").expect("parse agent path"),
                        session_id: "child-session".to_string(),
                        parent: AgentTurnRef {
                            session_id: "root-session".to_string(),
                            turn_id: "root-turn".to_string(),
                        },
                    },
                    agent_id: "reviewer".to_string(),
                    latest_update: None,
                    outcome: SubagentOutcome::Completed {
                        turn_id: "child-turn".to_string(),
                        output: "review complete".to_string(),
                        truncated: false,
                    },
                }),
            }],
            false,
        )
        .expect("project subagent result");

        assert_eq!(projection.summary, "/root/review · completed");
        assert_eq!(
            projection.output_lines,
            vec![
                "agent: reviewer".to_string(),
                "session: child-session".to_string(),
                "turn: child-turn".to_string(),
                "review complete".to_string(),
            ]
        );
    }

    #[test]
    fn projects_send_message_acknowledgement() {
        let subagent = SubagentRef {
            path: AgentPath::from_str("/root/review").expect("parse agent path"),
            session_id: "child-session".to_string(),
            parent: AgentTurnRef {
                session_id: "root-session".to_string(),
                turn_id: "root-turn".to_string(),
            },
        };
        let projection = project_subagent(
            "send_message",
            "call-message",
            r#"{"target":{"type":"directChild","name":"review"},"message":"focus"}"#,
            &[TalosContentItem::SubagentResult {
                result: CollaborationToolResult::SendMessage(SendMessageResult {
                    subagent,
                    agent_id: "reviewer".to_string(),
                    turn: AgentTurnRef {
                        session_id: "child-session".to_string(),
                        turn_id: "child-turn".to_string(),
                    },
                    accepted: true,
                }),
            }],
            false,
        )
        .expect("project send message result");

        assert_eq!(projection.summary, "message accepted by /root/review");
        assert_eq!(
            projection.preview_lines,
            vec![
                "agent: /root/review".to_string(),
                "agent id: reviewer".to_string()
            ]
        );
    }

    #[test]
    fn labels_every_server_outcome_without_deriving_state() {
        let outcomes = [
            SubagentOutcome::Running {
                turn_id: "turn".to_string(),
            },
            SubagentOutcome::WaitingUserAction {
                turn_id: "turn".to_string(),
            },
            SubagentOutcome::Cancelling {
                turn_id: "turn".to_string(),
            },
            SubagentOutcome::Completed {
                turn_id: "turn".to_string(),
                output: String::new(),
                truncated: false,
            },
            SubagentOutcome::Failed {
                turn_id: "turn".to_string(),
                error: "failed".to_string(),
                truncated: false,
            },
            SubagentOutcome::Interrupted {
                turn_id: "turn".to_string(),
            },
        ];

        assert_eq!(
            outcomes.map(|outcome| subagent_status_label(&outcome)),
            [
                "running",
                "waiting for user action",
                "cancelling",
                "completed",
                "failed",
                "interrupted",
            ]
        );
    }
}
