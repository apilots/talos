use std::sync::Arc;

use async_trait::async_trait;
use schemars::JsonSchema;
use serde_json::Value;
use talos_error::{TalosError, TalosResult};
use talos_types::{
    AgentCaller, CollaborationToolResult, FollowUpSubagentRequest, FollowupTaskArguments,
    InterruptAgentArguments, InterruptSubagentRequest, ListAgentsArguments, ListSubagentsRequest,
    SendMessageArguments, SendSubagentMessageRequest, SpawnAgentArguments, SpawnSubagentRequest,
    WaitAgentArguments, WaitSubagentRequest,
};

use crate::spec::{ToolBehaviorFlags, ToolResultPolicy, ToolSource};
use crate::{decode_tool_input, Tool, ToolContext, ToolOutputPayload};

const MAX_COLLABORATION_RESULT_BYTES: usize = 32 * 1024;

struct SpawnAgentTool;
struct SendMessageTool;
struct WaitAgentTool;
struct InterruptAgentTool;
struct FollowupTaskTool;
struct ListAgentsTool;

pub(crate) fn subagent_tools() -> Vec<Arc<dyn Tool>> {
    vec![
        Arc::new(SpawnAgentTool),
        Arc::new(SendMessageTool),
        Arc::new(WaitAgentTool),
        Arc::new(InterruptAgentTool),
        Arc::new(FollowupTaskTool),
        Arc::new(ListAgentsTool),
    ]
}

#[async_trait]
impl Tool for SpawnAgentTool {
    fn name(&self) -> &str {
        "spawn_agent"
    }

    fn description(&self) -> &str {
        "Start one subagent for a bounded delegated task."
    }

    fn parameters_schema(&self) -> Option<Value> {
        arguments_schema::<SpawnAgentArguments>()
    }

    fn source(&self) -> ToolSource {
        ToolSource::Collaboration
    }

    fn behavior_flags(&self) -> ToolBehaviorFlags {
        mutating_flags(false)
    }

    fn result_policy(&self) -> ToolResultPolicy {
        collaboration_result_policy()
    }

    async fn execute_payload(
        &self,
        input: Value,
        context: ToolContext,
    ) -> TalosResult<ToolOutputPayload> {
        let input: SpawnAgentArguments = decode_tool_input(input, self.name())?;
        let result = control(&context)?
            .spawn(SpawnSubagentRequest {
                caller: trusted_caller(&context)?,
                task_name: input.task_name,
                prompt: input.prompt,
            })
            .await?;
        collaboration_payload(CollaborationToolResult::SpawnAgent(result))
    }
}

#[async_trait]
impl Tool for SendMessageTool {
    fn name(&self) -> &str {
        "send_message"
    }

    fn description(&self) -> &str {
        "Send a message to one running subagent."
    }

    fn parameters_schema(&self) -> Option<Value> {
        arguments_schema::<SendMessageArguments>()
    }

    fn source(&self) -> ToolSource {
        ToolSource::Collaboration
    }

    fn behavior_flags(&self) -> ToolBehaviorFlags {
        mutating_flags(false)
    }

    fn result_policy(&self) -> ToolResultPolicy {
        collaboration_result_policy()
    }

    async fn execute_payload(
        &self,
        input: Value,
        context: ToolContext,
    ) -> TalosResult<ToolOutputPayload> {
        let input: SendMessageArguments = decode_tool_input(input, self.name())?;
        let result = control(&context)?
            .send_message(SendSubagentMessageRequest {
                caller: trusted_caller(&context)?,
                target: input.target,
                message: input.message,
            })
            .await?;
        collaboration_payload(CollaborationToolResult::SendMessage(result))
    }
}

#[async_trait]
impl Tool for WaitAgentTool {
    fn name(&self) -> &str {
        "wait_agent"
    }

    fn description(&self) -> &str {
        "Wait until one selected subagent finishes or the timeout expires."
    }

    fn parameters_schema(&self) -> Option<Value> {
        arguments_schema::<WaitAgentArguments>()
    }

    fn source(&self) -> ToolSource {
        ToolSource::Collaboration
    }

    fn behavior_flags(&self) -> ToolBehaviorFlags {
        read_only_flags()
    }

    fn result_policy(&self) -> ToolResultPolicy {
        collaboration_result_policy()
    }

    async fn execute_payload(
        &self,
        input: Value,
        context: ToolContext,
    ) -> TalosResult<ToolOutputPayload> {
        let input: WaitAgentArguments = decode_tool_input(input, self.name())?;
        let caller = trusted_caller(&context)?;
        let result = control(&context)?
            .wait(WaitSubagentRequest {
                caller,
                target: input.target,
                timeout_ms: u64::from(input.timeout_ms),
            })
            .await?;
        collaboration_payload(CollaborationToolResult::WaitAgent(result))
    }
}

#[async_trait]
impl Tool for InterruptAgentTool {
    fn name(&self) -> &str {
        "interrupt_agent"
    }

    fn description(&self) -> &str {
        "Interrupt one running subagent."
    }

    fn parameters_schema(&self) -> Option<Value> {
        arguments_schema::<InterruptAgentArguments>()
    }

    fn source(&self) -> ToolSource {
        ToolSource::Collaboration
    }

    fn behavior_flags(&self) -> ToolBehaviorFlags {
        mutating_flags(true)
    }

    fn result_policy(&self) -> ToolResultPolicy {
        collaboration_result_policy()
    }

    async fn execute_payload(
        &self,
        input: Value,
        context: ToolContext,
    ) -> TalosResult<ToolOutputPayload> {
        let input: InterruptAgentArguments = decode_tool_input(input, self.name())?;
        let result = control(&context)?
            .interrupt(InterruptSubagentRequest {
                caller: trusted_caller(&context)?,
                target: input.target,
            })
            .await?;
        collaboration_payload(CollaborationToolResult::InterruptAgent(result))
    }
}

#[async_trait]
impl Tool for FollowupTaskTool {
    fn name(&self) -> &str {
        "followup_task"
    }

    fn description(&self) -> &str {
        "Start a follow-up turn on one finished subagent."
    }

    fn parameters_schema(&self) -> Option<Value> {
        arguments_schema::<FollowupTaskArguments>()
    }

    fn source(&self) -> ToolSource {
        ToolSource::Collaboration
    }

    fn behavior_flags(&self) -> ToolBehaviorFlags {
        mutating_flags(false)
    }

    fn result_policy(&self) -> ToolResultPolicy {
        collaboration_result_policy()
    }

    async fn execute_payload(
        &self,
        input: Value,
        context: ToolContext,
    ) -> TalosResult<ToolOutputPayload> {
        let input: FollowupTaskArguments = decode_tool_input(input, self.name())?;
        let result = control(&context)?
            .follow_up(FollowUpSubagentRequest {
                caller: trusted_caller(&context)?,
                target: input.target,
                prompt: input.prompt,
            })
            .await?;
        collaboration_payload(CollaborationToolResult::FollowupTask(result))
    }
}

#[async_trait]
impl Tool for ListAgentsTool {
    fn name(&self) -> &str {
        "list_agents"
    }

    fn description(&self) -> &str {
        "List the current Agent tree for this collaboration root."
    }

    fn parameters_schema(&self) -> Option<Value> {
        arguments_schema::<ListAgentsArguments>()
    }

    fn source(&self) -> ToolSource {
        ToolSource::Collaboration
    }

    fn behavior_flags(&self) -> ToolBehaviorFlags {
        read_only_flags()
    }

    fn result_policy(&self) -> ToolResultPolicy {
        collaboration_result_policy()
    }

    async fn execute_payload(
        &self,
        input: Value,
        context: ToolContext,
    ) -> TalosResult<ToolOutputPayload> {
        let _input: ListAgentsArguments = decode_tool_input(input, self.name())?;
        let result = control(&context)?
            .list(ListSubagentsRequest {
                caller: trusted_caller(&context)?,
            })
            .await?;
        collaboration_payload(CollaborationToolResult::ListAgents(result))
    }
}

fn control(context: &ToolContext) -> TalosResult<Arc<dyn crate::AgentControl>> {
    context.agent_control().ok_or_else(|| {
        TalosError::public(
            talos_error::ErrorCode::ToolExecutionFailed,
            "subagent control is not configured".to_string(),
        )
    })
}

fn trusted_caller(context: &ToolContext) -> TalosResult<AgentCaller> {
    let turn_id = context.turn_id().ok_or_else(|| {
        TalosError::public(
            talos_error::ErrorCode::ToolExecutionFailed,
            "subagent tool requires an active parent turn".to_string(),
        )
    })?;
    let agent_id = context.agent_id().ok_or_else(|| {
        TalosError::public(
            talos_error::ErrorCode::ToolExecutionFailed,
            "subagent tool requires an active parent agent".to_string(),
        )
    })?;
    Ok(AgentCaller {
        session_id: context.session_id().to_string(),
        turn_id: turn_id.to_string(),
        agent_id: agent_id.to_string(),
    })
}

fn collaboration_payload(result: CollaborationToolResult) -> TalosResult<ToolOutputPayload> {
    let structured_content = serde_json::to_value(&result).map_err(|err| {
        TalosError::public(
            talos_error::ErrorCode::ToolExecutionFailed,
            format!("serialize subagent tool result: {err}"),
        )
    })?;
    let encoded = serde_json::to_vec(&structured_content).map_err(|err| {
        TalosError::public(
            talos_error::ErrorCode::ToolExecutionFailed,
            format!("encode subagent tool result: {err}"),
        )
    })?;
    if encoded.len() > MAX_COLLABORATION_RESULT_BYTES {
        return Err(TalosError::public(
            talos_error::ErrorCode::ToolExecutionFailed,
            format!("subagent tool result exceeds the {MAX_COLLABORATION_RESULT_BYTES} byte limit"),
        ));
    }
    Ok(ToolOutputPayload::success_with_content_items(
        structured_content,
        vec![talos_types::talos::TalosContentItem::SubagentResult { result }],
    ))
}

fn collaboration_result_policy() -> ToolResultPolicy {
    ToolResultPolicy {
        max_inline_bytes: MAX_COLLABORATION_RESULT_BYTES,
        ..ToolResultPolicy::default()
    }
}

fn read_only_flags() -> ToolBehaviorFlags {
    ToolBehaviorFlags {
        read_only: true,
        destructive: false,
        concurrency_safe: false,
        requires_user_interaction: false,
    }
}

fn mutating_flags(destructive: bool) -> ToolBehaviorFlags {
    ToolBehaviorFlags {
        read_only: false,
        destructive,
        concurrency_safe: false,
        requires_user_interaction: false,
    }
}

fn arguments_schema<T: JsonSchema>() -> Option<Value> {
    Some(
        serde_json::to_value(schemars::schema_for!(T))
            .expect("statically derived Tool argument schema must serialize"),
    )
}

#[cfg(test)]
#[path = "subagent_tests.rs"]
mod tests;
