use super::*;

fn build_continuity_last_failure(executed: &ExecutedTurnArtifacts) -> Option<String> {
    executed
        .tool_result_log
        .iter()
        .rev()
        .find(|log| log.is_error)
        .map(|log| format!("{} returned an error", log.tool))
}

fn build_continuity_suggested_next_step(
    last_failure: Option<&str>,
    verified_paths: &[talos_prompt::VerifiedPathEvidence],
    tool_summaries: &[talos_prompt::ContinuityToolSummary],
) -> String {
    if last_failure.is_some() {
        "adjust the failing path, command, or tool arguments before retrying".to_string()
    } else if !verified_paths.is_empty() {
        "answer from the verified evidence without re-reading the same paths".to_string()
    } else if tool_summaries.is_empty() {
        "answer directly unless new evidence is required".to_string()
    } else {
        "continue with a narrower follow-up tool step".to_string()
    }
}

fn build_continuity_pending_tasks(
    last_failure: Option<&str>,
    verified_paths: &[talos_prompt::VerifiedPathEvidence],
    tool_summaries: &[talos_prompt::ContinuityToolSummary],
) -> Vec<String> {
    if last_failure.is_some() {
        vec!["fix the failing tool step and retry".to_string()]
    } else if !verified_paths.is_empty() {
        vec!["complete the answer from the verified evidence".to_string()]
    } else if tool_summaries.is_empty() {
        Vec::new()
    } else {
        vec!["continue with a narrower follow-up tool step".to_string()]
    }
}

pub(crate) fn build_session_continuity(
    input: &str,
    executed: &ExecutedTurnArtifacts,
) -> Option<SessionContinuity> {
    let recent_goal = input.trim().to_string();
    let failed_call_ids = executed
        .tool_result_log
        .iter()
        .filter(|log| log.is_error)
        .map(|log| log.call_id.clone())
        .collect::<std::collections::HashSet<_>>();

    let mut verified_paths: Vec<talos_prompt::VerifiedPathEvidence> = Vec::new();
    let mut tool_summaries: Vec<talos_prompt::ContinuityToolSummary> = Vec::new();
    let mut grep_patterns = Vec::new();

    for call in &executed.tool_call_log {
        let tool_name = call.name.clone();
        if failed_call_ids.contains(&call.call_id) {
            if !tool_summaries.iter().any(|summary| {
                summary.tool_name == tool_name
                    && summary.outcome == talos_prompt::ContinuityToolOutcome::Failure
            }) {
                tool_summaries.push(talos_prompt::ContinuityToolSummary {
                    tool_name: tool_name.clone(),
                    outcome: talos_prompt::ContinuityToolOutcome::Failure,
                });
            }
        } else if !tool_summaries.iter().any(|summary| {
            summary.tool_name == tool_name
                && summary.outcome == talos_prompt::ContinuityToolOutcome::Success
        }) {
            tool_summaries.push(talos_prompt::ContinuityToolSummary {
                tool_name: tool_name.clone(),
                outcome: talos_prompt::ContinuityToolOutcome::Success,
            });
        }

        for key in ["file_path", "dir_path", "path"] {
            if let Some(path) = call
                .arguments
                .as_inner()
                .get(key)
                .and_then(|value| value.as_str())
            {
                if !verified_paths
                    .iter()
                    .any(|evidence: &talos_prompt::VerifiedPathEvidence| evidence.path == path)
                {
                    verified_paths.push(talos_prompt::VerifiedPathEvidence {
                        path: path.to_string(),
                        source_tool: tool_name.clone(),
                    });
                }
                break;
            }
        }

        if matches!(call.name.as_str(), "Grep" | "grep") {
            if let Some(pattern) = call
                .arguments
                .as_inner()
                .get("pattern")
                .and_then(|value| value.as_str())
            {
                grep_patterns.push(pattern.to_string());
            }
        }
    }

    let mut recent_unhelpful_patterns = Vec::new();
    if tool_summaries
        .iter()
        .any(|summary| summary.outcome == talos_prompt::ContinuityToolOutcome::Failure)
    {
        recent_unhelpful_patterns
            .push("avoid retrying the same failing path or command".to_string());
    }
    if grep_patterns.len() > 1 {
        let unique_patterns = grep_patterns
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len();
        if unique_patterns < grep_patterns.len() {
            recent_unhelpful_patterns.push(
                "avoid repeating grep on the same pattern without narrowing scope".to_string(),
            );
        }
    }
    if executed.tool_call_log.len() > 1
        && verified_paths.len() < executed.tool_call_log.len().min(3)
    {
        recent_unhelpful_patterns
            .push("avoid re-reading the same path when no new evidence is needed".to_string());
    }

    let recent_answer_mode = if tool_summaries.iter().any(|summary| {
        summary.outcome == talos_prompt::ContinuityToolOutcome::Success
            && matches!(summary.tool_name.as_str(), "Grep" | "Read" | "Glob")
    }) {
        Some(ContinuityAnswerMode::CodeFirst)
    } else if executed.tool_call_log.is_empty() {
        Some(ContinuityAnswerMode::DirectAnswer)
    } else {
        Some(ContinuityAnswerMode::Mixed)
    };

    let last_failure = build_continuity_last_failure(executed);
    let suggested_next_step = Some(build_continuity_suggested_next_step(
        last_failure.as_deref(),
        &verified_paths,
        &tool_summaries,
    ));
    let pending_tasks =
        build_continuity_pending_tasks(last_failure.as_deref(), &verified_paths, &tool_summaries);

    if recent_goal.is_empty()
        && verified_paths.is_empty()
        && tool_summaries.is_empty()
        && pending_tasks.is_empty()
        && recent_unhelpful_patterns.is_empty()
        && recent_answer_mode.is_none()
        && last_failure.is_none()
    {
        None
    } else {
        Some(SessionContinuity {
            recent_goal,
            verified_paths: verified_paths.into_iter().take(4).collect(),
            tool_summaries: tool_summaries.into_iter().take(4).collect(),
            pending_tasks: pending_tasks.into_iter().take(2).collect(),
            recent_unhelpful_patterns: recent_unhelpful_patterns.into_iter().take(3).collect(),
            recent_answer_mode,
            last_failure,
            suggested_next_step,
        })
    }
}
