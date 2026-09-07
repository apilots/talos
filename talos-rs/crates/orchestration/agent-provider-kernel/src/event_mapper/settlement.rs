use talos_agent::AgentResult;
use talos_agent_api::{
    AgentAssistantTextPhase, AgentContextCompactionDelta, AgentMessageBlockDeltaContent,
    AgentMessageBlockStatus, AgentMessageStatus,
};
use talos_error::TalosError;
use talos_types::{ContextCompactionOutcome, ContextCompactionStage};

use super::KernelAgentEventMapper;

impl KernelAgentEventMapper {
    pub(crate) async fn finish_completed(&mut self) -> AgentResult<()> {
        self.close_reasoning_with(AgentMessageBlockStatus::Completed)
            .await?;
        for key in self.assistant.keys().cloned().collect::<Vec<_>>() {
            let block = self.assistant_block(&key)?.clone();
            if block.phase == AgentAssistantTextPhase::Pending {
                self.sink
                    .append_block_delta(
                        &block.handle,
                        AgentMessageBlockDeltaContent::AssistantTextPhase(
                            AgentAssistantTextPhase::FinalAnswer,
                        ),
                    )
                    .await?;
            }
            self.sink
                .complete_block(&block.handle, AgentMessageBlockStatus::Completed)
                .await?;
            self.assistant.remove(&key);
        }
        if let Some(block) = self.compaction.clone() {
            self.sink
                .complete_block(&block, AgentMessageBlockStatus::Completed)
                .await?;
            self.compaction = None;
        }
        for call_id in self.tools_by_call.keys().cloned().collect::<Vec<_>>() {
            let tool = self.tools_by_call.get(&call_id).cloned().ok_or_else(|| {
                TalosError::new(
                    talos_error::ErrorCode::Conflict,
                    "Kernel Tool block disappeared during settlement",
                )
            })?;
            self.sink
                .complete_block(&tool.handle, AgentMessageBlockStatus::Failed)
                .await?;
            self.tools_by_call.remove(&call_id);
            self.tools_by_stream
                .retain(|_, current| current.call_id != call_id);
        }
        if let Some(message) = self.agent_message.clone() {
            self.sink
                .complete_message(&message, AgentMessageStatus::Completed)
                .await?;
            self.agent_message = None;
        }
        Ok(())
    }

    pub(crate) async fn finish_failed(&mut self, error: &TalosError) -> AgentResult<()> {
        let (block_status, message_status) = if is_interrupted(error) {
            (
                AgentMessageBlockStatus::Cancelled,
                AgentMessageStatus::Cancelled,
            )
        } else {
            (AgentMessageBlockStatus::Failed, AgentMessageStatus::Failed)
        };
        self.close_reasoning_with(block_status).await?;
        for key in self.assistant.keys().cloned().collect::<Vec<_>>() {
            let block = self.assistant_block(&key)?.clone();
            self.sink
                .complete_block(&block.handle, block_status)
                .await?;
            self.assistant.remove(&key);
        }
        if let Some(block) = self.compaction.clone() {
            self.sink
                .append_block_delta(
                    &block,
                    AgentMessageBlockDeltaContent::ContextCompaction(AgentContextCompactionDelta {
                        stage: Some(ContextCompactionStage::Failed),
                        outcome: (!is_interrupted(error))
                            .then_some(ContextCompactionOutcome::FailedInternalError),
                        metrics: None,
                        message: Some(error.to_api_error().message),
                    }),
                )
                .await?;
            self.sink.complete_block(&block, block_status).await?;
            self.compaction = None;
        }
        for call_id in self.tools_by_call.keys().cloned().collect::<Vec<_>>() {
            let tool = self.tools_by_call.get(&call_id).cloned().ok_or_else(|| {
                TalosError::new(
                    talos_error::ErrorCode::Conflict,
                    "Kernel Tool block disappeared during settlement",
                )
            })?;
            self.sink.complete_block(&tool.handle, block_status).await?;
            self.tools_by_call.remove(&call_id);
            self.tools_by_stream
                .retain(|_, current| current.call_id != call_id);
        }
        if let Some(message) = self.agent_message.clone() {
            self.sink.complete_message(&message, message_status).await?;
            self.agent_message = None;
        }
        Ok(())
    }

    pub(crate) async fn settle_failure(&mut self, error: TalosError) -> TalosError {
        match self.finish_failed(&error).await {
            Ok(()) => error,
            Err(settlement_error) => {
                talos_logging::report_error(&error);
                settlement_error.context("close failed Provider lifecycle")
            }
        }
    }
}

fn is_interrupted(error: &TalosError) -> bool {
    error.is_code(talos_error::ErrorCode::Cancelled)
}
