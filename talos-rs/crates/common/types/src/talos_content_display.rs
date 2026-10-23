use crate::subagent::CollaborationToolResult;
use crate::talos::TalosContentItem;

impl TalosContentItem {
    pub fn display_text(&self) -> String {
        match self {
            Self::Text { text } => text.clone(),
            Self::Json { value } => value.to_string(),
            Self::SubagentResult { result } => match result {
                CollaborationToolResult::SpawnAgent(_) => "spawn_agent result".to_string(),
                CollaborationToolResult::SendMessage(_) => "send_message result".to_string(),
                CollaborationToolResult::WaitAgent(_) => "wait_agent result".to_string(),
                CollaborationToolResult::InterruptAgent(_) => "interrupt_agent result".to_string(),
                CollaborationToolResult::FollowupTask(_) => "followup_task result".to_string(),
                CollaborationToolResult::ListAgents(_) => "list_agents result".to_string(),
            },
            Self::FileRead { content, .. } => content.clone(),
            Self::Diff { files } => format!("diff: {} file(s)", files.len()),
            Self::SearchResults {
                matches,
                more_available,
                partial,
                ..
            } => {
                let suffix = if *partial {
                    " partial"
                } else if *more_available {
                    " more_available"
                } else {
                    ""
                };
                format!("search_results: {} match(es){suffix}", matches.len())
            }
            Self::BashOutput {
                stdout,
                stderr,
                exit_code,
                status,
                output_log,
                ..
            } => {
                let mut parts = Vec::new();
                if !stdout.trim().is_empty() {
                    parts.push(stdout.trim().to_string());
                }
                if !stderr.trim().is_empty() {
                    parts.push(stderr.trim().to_string());
                }
                if parts.is_empty() {
                    parts.push(format!("bash: {:?} exit_code={exit_code:?}", status));
                }
                if let Some(log) = output_log {
                    parts.push(format!("Full output: {} ({} of {} bytes saved, {:?}; expires at {}). Use Grep or Read with offset/limit.",
                        log.path, log.stored_bytes, log.total_bytes, log.capture_status, log.expires_at));
                }
                parts.join("\n")
            }
            Self::WebReference {
                url,
                title,
                citation_id,
                snippet,
                ..
            } => format!(
                "web_reference: {}{}{}{}",
                title.as_deref().unwrap_or(url),
                title
                    .as_ref()
                    .map(|_| format!(" ({url})"))
                    .unwrap_or_default(),
                citation_id
                    .as_ref()
                    .map(|id| format!(" [{id}]"))
                    .unwrap_or_default(),
                snippet
                    .as_ref()
                    .filter(|value| !value.trim().is_empty())
                    .map(|value| format!("\n{value}"))
                    .unwrap_or_default()
            ),
            Self::Image { file_id, label, .. } => format!("image: {label} (file {file_id})"),
            Self::ArtifactRef {
                artifact_id,
                label,
                mime_type,
            } => format!(
                "artifact: {}{}",
                label.as_deref().unwrap_or(artifact_id),
                mime_type
                    .as_ref()
                    .map(|mime| format!(" ({mime})"))
                    .unwrap_or_default()
            ),
            Self::McpResourceRef {
                server,
                uri,
                mime_type,
            } => format!(
                "mcp_resource: {server}:{uri}{}",
                mime_type
                    .as_ref()
                    .map(|mime| format!(" ({mime})"))
                    .unwrap_or_default()
            ),
            Self::Progress { message, .. } => message.clone(),
        }
    }
}
