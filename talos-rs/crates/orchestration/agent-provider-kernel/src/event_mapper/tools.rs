use talos_agent::AgentResult;
use talos_agent_api::{
    AgentMessageBlockContent, AgentMessageBlockDeltaContent, AgentMessageBlockStatus,
    AgentMessageStatus, AgentToolCall, AgentToolCallArguments, AgentToolProgress, AgentToolResult,
};
use talos_agent_kernel::{ToolResultStreamData, ToolStreamKey};
use talos_types::{TalosToolProgressKind, ToolArguments};

use super::{KernelAgentEventMapper, ToolBlock};

impl KernelAgentEventMapper {
    pub(super) async fn start_tool(
        &mut self,
        stream_key: ToolStreamKey,
        call_id: String,
        tool_name: String,
        arguments: ToolArguments,
    ) -> AgentResult<ToolBlock> {
        self.close_reasoning().await?;
        if let Some(tool) = self.tools_by_call.get(&call_id) {
            self.tools_by_stream.insert(stream_key, tool.clone());
            return Ok(tool.clone());
        }
        let message = self.agent_message().await?;
        let block = self
            .sink
            .start_block(
                &message,
                AgentMessageBlockContent::ToolCall(AgentToolCall {
                    call_id: call_id.clone(),
                    name: tool_name,
                    arguments: AgentToolCallArguments::Complete { value: arguments },
                    progress: Vec::new(),
                }),
            )
            .await?;
        self.pending_tool_arguments.remove(&stream_key);
        let tool = ToolBlock {
            handle: block,
            call_id: call_id.clone(),
        };
        self.tools_by_stream.insert(stream_key, tool.clone());
        self.tools_by_call.insert(call_id, tool.clone());
        Ok(tool)
    }

    pub(super) async fn ensure_tool(
        &mut self,
        stream_key: ToolStreamKey,
        call_id: String,
        tool_name: String,
    ) -> AgentResult<ToolBlock> {
        if let Some(tool) = self.tools_by_call.get(&call_id) {
            self.tools_by_stream.insert(stream_key, tool.clone());
            return Ok(tool.clone());
        }
        let fragment = self
            .pending_tool_arguments
            .remove(&stream_key)
            .unwrap_or_else(|| "{}".to_string());
        self.close_reasoning().await?;
        let message = self.agent_message().await?;
        let block = self
            .sink
            .start_block(
                &message,
                AgentMessageBlockContent::ToolCall(AgentToolCall {
                    call_id: call_id.clone(),
                    name: tool_name,
                    arguments: AgentToolCallArguments::Streaming {
                        json_fragment: fragment,
                    },
                    progress: Vec::new(),
                }),
            )
            .await?;
        let tool = ToolBlock {
            handle: block,
            call_id: call_id.clone(),
        };
        self.tools_by_stream.insert(stream_key, tool.clone());
        self.tools_by_call.insert(call_id, tool.clone());
        Ok(tool)
    }

    pub(super) async fn append_tool_progress(
        &self,
        tool: &ToolBlock,
        kind: TalosToolProgressKind,
        text: String,
        occurred_at: i64,
    ) -> AgentResult<()> {
        self.sink
            .append_block_delta(
                &tool.handle,
                AgentMessageBlockDeltaContent::ToolProgress(AgentToolProgress {
                    kind,
                    text,
                    occurred_at,
                }),
            )
            .await
    }

    pub(super) async fn map_tool_result(&mut self, data: ToolResultStreamData) -> AgentResult<()> {
        self.close_reasoning().await?;
        let tool = self
            .tools_by_call
            .get(&data.call_id)
            .cloned()
            .ok_or_else(|| {
                talos_error::TalosError::new(
                    talos_error::ErrorCode::Conflict,
                    "Kernel Tool result has no Tool call",
                )
            })?;
        self.sink
            .complete_block(
                &tool.handle,
                if data.is_error {
                    AgentMessageBlockStatus::Failed
                } else {
                    AgentMessageBlockStatus::Completed
                },
            )
            .await?;
        self.tools_by_call.remove(&data.call_id);
        self.tools_by_stream
            .retain(|_, current| current.call_id != tool.call_id);

        let message = self
            .sink
            .start_message(talos_agent_api::AgentMessageRole::Tool)
            .await?;
        let block = self
            .sink
            .start_block(
                &message,
                AgentMessageBlockContent::ToolResult(AgentToolResult {
                    call_id: data.call_id,
                    tool_name: data.tool_name,
                    content: data.content,
                    is_error: data.is_error,
                }),
            )
            .await?;
        self.sink
            .complete_block(
                &block,
                if data.is_error {
                    AgentMessageBlockStatus::Failed
                } else {
                    AgentMessageBlockStatus::Completed
                },
            )
            .await?;
        self.sink
            .complete_message(
                &message,
                if data.is_error {
                    AgentMessageStatus::Failed
                } else {
                    AgentMessageStatus::Completed
                },
            )
            .await
    }
}
