use crate::CoordinatorImpl;
use std::sync::Arc;
use talos_agent::{AgentOrchestrator, OrchestratorEvent};
use talos_error::TalosResult;
use talos_types::talos::{
    TalosContext, TalosEventPayload, TalosEventType, TalosMessageCompletedEvent,
    TalosMessageErrorEvent, TalosMessageExecutionSummaryEvent, TalosMessageOutputDeltaEvent,
    TalosMessageReasoningDeltaEvent, TalosMessageStartedEvent, TalosMessageToolCallCompletedEvent,
    TalosMessageToolCallPendingApprovalEvent, TalosMessageToolCallStartedEvent,
};
use talos_types::TalosSession;

pub(super) fn route_channel(context: &TalosContext) -> String {
    match context.channel_id.as_deref() {
        Some(channel_id) => format!("{}:{}", context.provider, channel_id),
        None => context.provider.to_string(),
    }
}

impl CoordinatorImpl {
    pub(super) async fn resolve_session_agent(
        &self,
        session: &TalosSession,
    ) -> TalosResult<(String, Arc<dyn AgentOrchestrator>)> {
        let route_context = TalosContext {
            source_type: session.context.source_type.clone(),
            provider: session.context.provider.clone(),
            user_id: session.context.user_id.clone(),
            message_id: session.context.message_id.clone(),
            trace_id: session.context.trace_id.clone(),
            channel_id: session.context.channel_id.clone(),
            thread_id: session.context.thread_id.clone(),
            environment: session.context.environment.clone(),
        };
        let route_channel = route_channel(&route_context);
        let target_agent = self
            .router
            .route(&route_channel, &route_context.user_id)
            .await?
            .unwrap_or_else(|| "default".to_string());
        let agents = self.agents.read().await;
        let agent = agents
            .get(&target_agent)
            .cloned()
            .ok_or_else(|| talos_error::TalosError::AgentNotFound(target_agent.clone()))?;
        Ok((target_agent, agent))
    }
}

pub(super) fn map_orchestrator_event(
    event: OrchestratorEvent,
) -> (TalosEventType, TalosEventPayload) {
    match event {
        OrchestratorEvent::MessageStarted(payload) => (
            TalosEventType::MessageStarted,
            TalosEventPayload::MessageStarted(TalosMessageStartedEvent {
                request_id: payload.request_id,
                model: payload.model,
                usage: payload.usage,
                iteration: payload.iteration,
                step_id: payload.step_id,
            }),
        ),
        OrchestratorEvent::MessageReasoningDelta(payload) => (
            TalosEventType::MessageReasoningDelta,
            TalosEventPayload::MessageReasoningDelta(TalosMessageReasoningDeltaEvent {
                request_id: payload.request_id,
                delta: payload.delta,
                source: payload.source,
                stage: payload.stage,
                item_index: payload.item_index,
                iteration: payload.iteration,
                step_id: payload.step_id,
            }),
        ),
        OrchestratorEvent::MessageExecutionSummary(payload) => (
            TalosEventType::MessageExecutionSummary,
            TalosEventPayload::MessageExecutionSummary(TalosMessageExecutionSummaryEvent {
                request_id: payload.request_id,
                text: payload.text,
                item_index: payload.item_index,
                iteration: payload.iteration,
                step_id: payload.step_id,
            }),
        ),
        OrchestratorEvent::MessageOutputDelta(payload) => (
            TalosEventType::MessageOutputDelta,
            TalosEventPayload::MessageOutputDelta(TalosMessageOutputDeltaEvent {
                request_id: payload.request_id,
                delta: payload.delta,
                item_index: payload.item_index,
                iteration: payload.iteration,
                step_id: payload.step_id,
            }),
        ),
        OrchestratorEvent::MessageToolCallStarted(payload) => (
            TalosEventType::MessageToolCallStarted,
            TalosEventPayload::MessageToolCallStarted(TalosMessageToolCallStartedEvent {
                request_id: payload.request_id,
                call_id: payload.call_id,
                name: payload.name,
                arguments: payload.arguments,
                item_index: payload.item_index,
                iteration: payload.iteration,
                step_id: payload.step_id,
                timing_ms: payload.timing_ms,
            }),
        ),
        OrchestratorEvent::MessageToolCallCompleted(payload) => (
            TalosEventType::MessageToolCallCompleted,
            TalosEventPayload::MessageToolCallCompleted(TalosMessageToolCallCompletedEvent {
                request_id: payload.request_id,
                call_id: payload.call_id,
                tool: payload.tool,
                content: payload.content,
                is_error: payload.is_error,
                item_index: payload.item_index,
                iteration: payload.iteration,
                step_id: payload.step_id,
                timing_ms: payload.timing_ms,
            }),
        ),
        OrchestratorEvent::MessageToolCallPendingApproval(payload) => (
            TalosEventType::MessageToolCallPendingApproval,
            TalosEventPayload::MessageToolCallPendingApproval(
                TalosMessageToolCallPendingApprovalEvent {
                    request_id: payload.request_id,
                    call_id: payload.call_id,
                    tool_name: payload.tool_name,
                    prompt: payload.prompt,
                    arguments: payload.arguments,
                    step_id: payload.step_id,
                },
            ),
        ),
        OrchestratorEvent::MessageCompleted(payload) => (
            TalosEventType::MessageCompleted,
            TalosEventPayload::MessageCompleted(TalosMessageCompletedEvent {
                request_id: payload.request_id,
                stop_reason: payload.stop_reason,
                usage: payload.usage,
                total_output_items: payload.total_output_items,
                iteration: payload.iteration,
                step_id: payload.step_id,
                task_count: payload.task_count,
            }),
        ),
        OrchestratorEvent::MessageError(payload) => (
            TalosEventType::MessageError,
            TalosEventPayload::MessageError(TalosMessageErrorEvent {
                request_id: payload.request_id,
                error: payload.error,
            }),
        ),
    }
}
