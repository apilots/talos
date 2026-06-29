use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use talos_error::{TalosError, TalosResult};
use talos_types::talos::StopReason;

use crate::spec::{ToolBehaviorFlags, ToolResultPolicy, ToolVisibility};
use crate::state::PermissionMode;
use crate::{decode_tool_input, CollaborationAgentStatus, Tool, ToolContext, ToolOutputPayload};

pub struct AgentTool;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum AgentKind {
    GeneralPurpose,
    Research,
    Implementation,
    Review,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum AgentExecutionMode {
    Foreground,
    Background,
}

#[derive(Debug, Deserialize)]
struct AgentRequest {
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    prompt: Option<String>,
    #[serde(default)]
    task: Option<String>,
    #[serde(default)]
    subagent_type: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    team_name: Option<String>,
    #[serde(default)]
    mode: Option<PermissionMode>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    run_in_background: Option<bool>,
    #[serde(default)]
    execution_mode: Option<AgentExecutionMode>,
    #[serde(default)]
    wait_timeout_ms: Option<u64>,
}

#[derive(Debug, Serialize)]
struct AgentResponse {
    session_id: String,
    agent_kind: AgentKind,
    execution_mode: AgentExecutionMode,
    status: AgentToolStatus,
    output: Option<String>,
    stop_reason: Option<StopReason>,
    error: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum AgentToolStatus {
    Running,
    Completed,
    Failed,
    Closed,
    NotFound,
}

#[async_trait]
impl Tool for AgentTool {
    fn name(&self) -> &str {
        "Agent"
    }

    fn internal_name(&self) -> &str {
        "agent"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["agent"]
    }

    fn description(&self) -> &str {
        "Launch a new agent.\n\nUsage:\n- description should be a short 3-5 word summary of the delegated task.\n- prompt should contain the full task for the child agent.\n- subagent_type can request a specialized agent for research, implementation, or review work.\n- Use this tool for parallelizable or self-contained work, not for simple file reads or single Grep calls.\n- Set run_in_background=true when the agent can continue asynchronously and you will check on it later."
    }

    fn visibility(&self) -> ToolVisibility {
        ToolVisibility::DefaultVisible
    }

    fn behavior_flags(&self) -> ToolBehaviorFlags {
        ToolBehaviorFlags {
            read_only: false,
            destructive: true,
            concurrency_safe: false,
            requires_user_interaction: false,
        }
    }

    fn result_policy(&self) -> ToolResultPolicy {
        ToolResultPolicy {
            max_inline_bytes: 32 * 1024,
            persist_large_output: true,
            preview_lines: 40,
        }
    }

    fn parameters_schema(&self) -> Option<Value> {
        Some(serde_json::json!({
            "type": "object",
            "required": ["description", "prompt"],
            "properties": {
                "description": {
                    "type": "string",
                    "description": "Short 3-5 word description of the task."
                },
                "prompt": {
                    "type": "string",
                    "description": "Complete task prompt for the child agent."
                },
                "subagent_type": {
                    "type": "string",
                    "description": "Optional specialized agent type to use."
                },
                "model": {
                    "type": "string",
                    "enum": ["sonnet", "opus", "haiku"],
                    "description": "Optional model override for the child agent."
                },
                "name": {
                    "type": "string",
                    "description": "Optional name for the spawned agent."
                },
                "team_name": {
                    "type": "string",
                    "description": "Optional team name for the spawned agent."
                },
                "mode": {
                    "type": "string",
                    "enum": ["default", "plan"],
                    "description": "Optional permission mode hint for the child agent."
                },
                "run_in_background": {
                    "type": "boolean",
                    "description": "Whether to run the agent asynchronously in the background."
                },
                "cwd": {
                    "type": "string",
                    "description": "Optional working directory hint that will be embedded into the child prompt."
                },
                "execution_mode": {
                    "type": "string",
                    "enum": ["foreground", "background"]
                },
                "wait_timeout_ms": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Foreground mode only. Maximum time to wait for a terminal child-agent result."
                }
            }
        }))
    }

    async fn execute_payload(
        &self,
        input: Value,
        context: ToolContext,
    ) -> TalosResult<ToolOutputPayload> {
        let request: AgentRequest = decode_tool_input(input, self.name())?;
        let task_prompt = request
            .prompt
            .as_deref()
            .or(request.task.as_deref())
            .map(str::trim)
            .unwrap_or_default();
        if task_prompt.is_empty() {
            return Err(TalosError::ValidationError(
                "prompt cannot be empty".to_string(),
            ));
        }

        let runtime = context.collaboration_runtime().ok_or_else(|| {
            TalosError::ToolError("collaboration runtime is not configured".to_string())
        })?;

        let agent_kind = classify_agent_kind(request.subagent_type.as_deref());
        let execution_mode = if request.run_in_background.unwrap_or(false) {
            AgentExecutionMode::Background
        } else {
            request
                .execution_mode
                .unwrap_or(AgentExecutionMode::Foreground)
        };
        let initial_message = build_agent_prompt(&request, agent_kind);
        let session_id = runtime
            .spawn_agent(context.user_id(), Some(initial_message))
            .await?;

        let response = match execution_mode {
            AgentExecutionMode::Background => AgentResponse {
                session_id,
                agent_kind,
                execution_mode,
                status: AgentToolStatus::Running,
                output: None,
                stop_reason: None,
                error: None,
            },
            AgentExecutionMode::Foreground => {
                let statuses = runtime
                    .wait(
                        std::slice::from_ref(&session_id),
                        request.wait_timeout_ms.unwrap_or(30_000),
                    )
                    .await?;
                let (_, status) = statuses.into_iter().next().ok_or_else(|| {
                    TalosError::ToolError("agent wait returned no status".to_string())
                })?;
                build_agent_response(session_id, agent_kind, execution_mode, status)
            }
        };

        ToolOutputPayload::success_json(&response, "serialize agent response")
    }
}

#[cfg(test)]
#[path = "agent_tests.rs"]
mod tests;

fn build_agent_prompt(request: &AgentRequest, agent_kind: AgentKind) -> String {
    let task_prompt = request
        .prompt
        .as_deref()
        .or(request.task.as_deref())
        .map(str::trim)
        .unwrap_or_default();
    let mut sections = vec![
        format!("Agent kind: {}", agent_kind_label(agent_kind)),
        format!("Task:\n{}", task_prompt),
    ];

    if let Some(description) = request.description.as_deref().map(str::trim) {
        if !description.is_empty() {
            sections.insert(1, format!("Description:\n{description}"));
        }
    }

    if let Some(subagent_type) = request.subagent_type.as_deref().map(str::trim) {
        if !subagent_type.is_empty() {
            sections.push(format!("Requested subagent type:\n{subagent_type}"));
        }
    }

    if let Some(model) = request.model.as_deref().map(str::trim) {
        if !model.is_empty() {
            sections.push(format!("Requested model:\n{model}"));
        }
    }

    if let Some(name) = request.name.as_deref().map(str::trim) {
        if !name.is_empty() {
            sections.push(format!("Requested agent name:\n{name}"));
        }
    }

    if let Some(team_name) = request.team_name.as_deref().map(str::trim) {
        if !team_name.is_empty() {
            sections.push(format!("Requested team name:\n{team_name}"));
        }
    }

    if let Some(mode) = request.mode {
        sections.push(format!(
            "Requested permission mode:\n{}",
            permission_mode_label(mode)
        ));
    }

    if let Some(cwd) = request.cwd.as_deref().map(str::trim) {
        if !cwd.is_empty() {
            sections.push(format!("Working directory hint:\n{cwd}"));
        }
    }

    sections.push(
        "Execution rules:\n- You are already the delegated child agent.\n- Do not call the Agent tool again unless the parent task explicitly requires further delegation.\n- If the task can be completed directly, answer directly instead of using tools.\n- For simple textual tasks, prefer returning the final answer without any tool call."
            .to_string(),
    );

    sections.join("\n\n")
}

fn build_agent_response(
    session_id: String,
    agent_kind: AgentKind,
    execution_mode: AgentExecutionMode,
    status: CollaborationAgentStatus,
) -> AgentResponse {
    match status {
        CollaborationAgentStatus::Idle | CollaborationAgentStatus::Running => AgentResponse {
            session_id,
            agent_kind,
            execution_mode,
            status: AgentToolStatus::Running,
            output: None,
            stop_reason: None,
            error: None,
        },
        CollaborationAgentStatus::Completed {
            output,
            stop_reason,
        } => AgentResponse {
            session_id,
            agent_kind,
            execution_mode,
            status: AgentToolStatus::Completed,
            output: Some(output),
            stop_reason: Some(stop_reason),
            error: None,
        },
        CollaborationAgentStatus::Failed { error } => AgentResponse {
            session_id,
            agent_kind,
            execution_mode,
            status: AgentToolStatus::Failed,
            output: None,
            stop_reason: None,
            error: Some(error),
        },
        CollaborationAgentStatus::Closed => AgentResponse {
            session_id,
            agent_kind,
            execution_mode,
            status: AgentToolStatus::Closed,
            output: None,
            stop_reason: None,
            error: None,
        },
        CollaborationAgentStatus::NotFound => AgentResponse {
            session_id,
            agent_kind,
            execution_mode,
            status: AgentToolStatus::NotFound,
            output: None,
            stop_reason: None,
            error: None,
        },
    }
}

fn agent_kind_label(agent_kind: AgentKind) -> &'static str {
    match agent_kind {
        AgentKind::GeneralPurpose => "general_purpose",
        AgentKind::Research => "research",
        AgentKind::Implementation => "implementation",
        AgentKind::Review => "review",
    }
}

fn classify_agent_kind(subagent_type: Option<&str>) -> AgentKind {
    match subagent_type.unwrap_or_default() {
        "research" => AgentKind::Research,
        "implementation" => AgentKind::Implementation,
        "review" => AgentKind::Review,
        _ => AgentKind::GeneralPurpose,
    }
}

fn permission_mode_label(mode: PermissionMode) -> &'static str {
    match mode {
        PermissionMode::Default => "default",
        PermissionMode::Plan => "plan",
    }
}
