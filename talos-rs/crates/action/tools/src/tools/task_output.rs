use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;
use talos_error::{TalosError, TalosResult};
use talos_types::talos::{TalosContentItem, TalosToolTaskStatus};

use crate::runtimes::bash::{read_task_output_with_context, TaskOutputRequest, TaskStatus};
use crate::spec::{ToolBehaviorFlags, ToolVisibility};
use crate::{Tool, ToolContext, ToolOutputPayload};

pub struct TaskOutputTool;

#[derive(Debug, Deserialize)]
struct TaskOutputInput {
    task_id: i64,
    #[serde(default)]
    offset: Option<u64>,
    #[serde(default)]
    max_output_tokens: Option<usize>,
}

#[async_trait]
impl Tool for TaskOutputTool {
    fn name(&self) -> &str {
        "TaskOutput"
    }

    fn internal_name(&self) -> &str {
        "task_output"
    }

    fn description(&self) -> &str {
        "Read output from a background task by task ID.\n\nUsage:\n- Use this for background Bash tasks launched with run_in_background=true.\n- Set block semantics through repeated polling from the caller when needed.\n- Prefer reading the task output file directly once task lifecycle reporting is available."
    }

    fn visibility(&self) -> ToolVisibility {
        ToolVisibility::DefaultVisible
    }

    fn parameters_schema(&self) -> Option<Value> {
        Some(serde_json::json!({
            "type": "object",
            "required": ["task_id"],
            "properties": {
                "task_id": { "type": "integer" },
                "offset": { "type": "integer", "minimum": 0 },
                "max_output_tokens": { "type": "integer", "minimum": 1 }
            }
        }))
    }

    fn behavior_flags(&self) -> ToolBehaviorFlags {
        ToolBehaviorFlags {
            read_only: true,
            destructive: false,
            concurrency_safe: true,
            requires_user_interaction: false,
        }
    }

    async fn execute_payload(
        &self,
        input: Value,
        context: ToolContext,
    ) -> TalosResult<ToolOutputPayload> {
        let input: TaskOutputInput = serde_json::from_value(input).map_err(|err| {
            TalosError::ToolError(format!("invalid TaskOutput arguments: {}", err))
        })?;
        let offset = input
            .offset
            .map(|value| {
                usize::try_from(value).map_err(|_| {
                    TalosError::ToolError(format!("TaskOutput offset is too large: {value}"))
                })
            })
            .transpose()?;
        let response = read_task_output_with_context(
            &TaskOutputRequest {
                session_id: input.task_id,
                chars: String::new(),
                yield_time_ms: 250,
                offset,
                max_output_tokens: input.max_output_tokens,
            },
            &context,
        )
        .await?;
        let structured = serde_json::to_value(&response).map_err(|err| {
            TalosError::ToolError(format!("serialize TaskOutput response: {err}"))
        })?;
        let status = match response.status {
            TaskStatus::Running => TalosToolTaskStatus::Running,
            TaskStatus::Stopped => TalosToolTaskStatus::Cancelled,
            TaskStatus::Exited => {
                if response.exit_code == Some(0) {
                    TalosToolTaskStatus::Completed
                } else {
                    TalosToolTaskStatus::Failed
                }
            }
        };
        let (stdout, stderr) = task_output_chunk(&response);
        Ok(ToolOutputPayload::success_with_content_items(
            structured,
            vec![TalosContentItem::BashOutput {
                session_id: response.session_id,
                status,
                exit_code: response.exit_code,
                stdout,
                stderr,
                truncated: response.truncated,
                elapsed_ms: Some(response.elapsed_ms),
            }],
        ))
    }
}

fn task_output_chunk(response: &crate::runtimes::bash::TaskSessionResponse) -> (String, String) {
    if response.output.is_empty() {
        return (String::new(), String::new());
    }

    if response.stderr.is_empty() && response.stdout.ends_with(&response.output) {
        return (response.output.clone(), String::new());
    }

    if response.stdout.is_empty() && response.stderr.ends_with(&response.output) {
        return (String::new(), response.output.clone());
    }

    (response.output.clone(), String::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtimes::bash::{
        run_bash_command, BashCommandRequest, SandboxPermissions, TaskStatus,
    };

    #[tokio::test]
    async fn test_task_output_tool_passes_offset_to_runtime() {
        let context = ToolContext::default()
            .with_project_root(std::env::current_dir().expect("cwd should exist"));
        let marker = "task-output-tool-offset-marker";
        let exec = run_bash_command(
            &BashCommandRequest {
                cmd: format!(
                    "python3 -c \"import time; print('x' * 1300 + '{}', flush=True); time.sleep(1)\"",
                    marker
                ),
                workdir: None,
                shell: Some("sh".to_string()),
                login: false,
                tty: false,
                yield_time_ms: 100,
                max_output_tokens: None,
                sandbox_permissions: SandboxPermissions::UseDefault,
                justification: None,
                prefix_rule: None,
            },
            &context,
        )
        .await
        .expect("bash command should start");

        assert_eq!(exec.status, TaskStatus::Running);
        let task_id = exec.session_id.expect("session id should be present");
        let payload = TaskOutputTool
            .execute_payload(
                serde_json::json!({
                    "task_id": task_id,
                    "offset": 1200,
                    "max_output_tokens": 4096
                }),
                context,
            )
            .await
            .expect("TaskOutput should execute");

        let ToolOutputPayload::Function {
            structured_content: Some(content),
            content_items,
            ..
        } = payload
        else {
            panic!("expected structured TaskOutput payload");
        };
        let output = content
            .get("output")
            .and_then(Value::as_str)
            .expect("output should be a string");
        assert!(output.contains(marker), "{output}");
        assert!(
            !output.starts_with(&"x".repeat(1200)),
            "offset output should not include the skipped prefix"
        );

        let Some(TalosContentItem::BashOutput {
            stdout, truncated, ..
        }) = content_items.first()
        else {
            panic!("expected BashOutput content item");
        };
        assert!(stdout.contains(marker), "{stdout}");
        assert!(
            !stdout.starts_with(&"x".repeat(1200)),
            "typed output should preserve TaskOutput offset"
        );
        assert!(!truncated);
    }
}
