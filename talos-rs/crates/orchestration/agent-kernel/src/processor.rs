use crate::{
    EventActions, EventAuthor, EventPayload, EventSink, ExecutionEvent, ExecutionEventKind,
    InvocationContext, LlmRequestEnvelope, LlmResponseEnvelope, Planner, ResponseDisposition,
};
use async_trait::async_trait;
use std::sync::Arc;
use talos_error::TalosResult;
use talos_types::talos::{TalosContentItem, TalosOutputItem};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum RequestProcessorKind {
    Basic = 0,
    Auth = 1,
    RequestConfirmation = 2,
    Instructions = 3,
    Identity = 4,
    Compaction = 5,
    Contents = 6,
    ContextCache = 7,
    Interactions = 8,
    Planning = 9,
    ToolContext = 10,
    AgentTransfer = 11,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ResponseProcessorKind {
    Planning = 0,
    ToolCallExtraction = 1,
    FinalAnswerExtraction = 2,
    StateDelta = 3,
    EventProjection = 4,
    CodeExecution = 5,
}

#[async_trait]
pub trait RequestProcessor: Send + Sync {
    fn kind(&self) -> RequestProcessorKind;

    async fn run(
        &self,
        _ctx: &mut InvocationContext,
        _request: &mut LlmRequestEnvelope,
        _sink: &mut dyn EventSink,
    ) -> TalosResult<()> {
        Ok(())
    }
}

#[async_trait]
pub trait ResponseProcessor: Send + Sync {
    fn kind(&self) -> ResponseProcessorKind;

    async fn run(
        &self,
        _ctx: &mut InvocationContext,
        _response: &mut LlmResponseEnvelope,
        _sink: &mut dyn EventSink,
    ) -> TalosResult<ResponseDisposition> {
        Ok(ResponseDisposition::Continue)
    }
}

pub struct NoopRequestProcessor {
    kind: RequestProcessorKind,
}

impl NoopRequestProcessor {
    pub fn new(kind: RequestProcessorKind) -> Self {
        Self { kind }
    }
}

#[async_trait]
impl RequestProcessor for NoopRequestProcessor {
    fn kind(&self) -> RequestProcessorKind {
        self.kind
    }
}

pub struct NoopResponseProcessor {
    kind: ResponseProcessorKind,
}

impl NoopResponseProcessor {
    pub fn new(kind: ResponseProcessorKind) -> Self {
        Self { kind }
    }
}

#[async_trait]
impl ResponseProcessor for NoopResponseProcessor {
    fn kind(&self) -> ResponseProcessorKind {
        self.kind
    }
}

pub struct PlanningRequestProcessor {
    planner: Arc<dyn Planner>,
}

impl PlanningRequestProcessor {
    pub fn new(planner: Arc<dyn Planner>) -> Self {
        Self { planner }
    }
}

#[async_trait]
impl RequestProcessor for PlanningRequestProcessor {
    fn kind(&self) -> RequestProcessorKind {
        RequestProcessorKind::Planning
    }

    async fn run(
        &self,
        ctx: &mut InvocationContext,
        request: &mut LlmRequestEnvelope,
        _sink: &mut dyn EventSink,
    ) -> TalosResult<()> {
        let instruction = self.planner.build_planning_instruction(ctx);
        ctx.planning_instruction = instruction.clone();
        request.planning_instruction = instruction;
        Ok(())
    }
}

pub struct PlanningResponseProcessor {
    planner: Arc<dyn Planner>,
}

impl PlanningResponseProcessor {
    pub fn new(planner: Arc<dyn Planner>) -> Self {
        Self { planner }
    }
}

#[async_trait]
impl ResponseProcessor for PlanningResponseProcessor {
    fn kind(&self) -> ResponseProcessorKind {
        ResponseProcessorKind::Planning
    }

    async fn run(
        &self,
        ctx: &mut InvocationContext,
        response: &mut LlmResponseEnvelope,
        sink: &mut dyn EventSink,
    ) -> TalosResult<ResponseDisposition> {
        let effect = self
            .planner
            .process_planning_response_effect(ctx, &response.output_text);

        if let Some(normalized) = effect.normalized_text {
            ctx.set_planning_response(normalized);
        }

        if !effect.state_delta.is_empty() || effect.branch_override.is_some() {
            if let Some(branch_override) = effect.branch_override {
                response.branch = branch_override;
            }
            sink.push({
                let mut event = ExecutionEvent::new(
                    ctx,
                    EventAuthor::Agent,
                    response.branch,
                    ExecutionEventKind::Planning,
                    EventPayload::Empty,
                    false,
                );
                event.actions = EventActions {
                    state_delta: effect.state_delta,
                    end_turn: false,
                    transfer_to: None,
                    branch_override: effect.branch_override,
                };
                event
            })
            .await?;
        }

        Ok(ResponseDisposition::Continue)
    }
}

pub struct ToolCallExtractionProcessor;

#[async_trait]
impl ResponseProcessor for ToolCallExtractionProcessor {
    fn kind(&self) -> ResponseProcessorKind {
        ResponseProcessorKind::ToolCallExtraction
    }

    async fn run(
        &self,
        ctx: &mut InvocationContext,
        response: &mut LlmResponseEnvelope,
        sink: &mut dyn EventSink,
    ) -> TalosResult<ResponseDisposition> {
        for item in &response.output_items {
            if let TalosOutputItem::ToolCall {
                call_id,
                name,
                arguments,
            } = item
            {
                sink.push(ExecutionEvent::tool_call(
                    ctx,
                    response.branch,
                    call_id.clone(),
                    name.clone(),
                    arguments.clone(),
                ))
                .await?;
            }
        }

        Ok(ResponseDisposition::Continue)
    }
}

pub struct FinalAnswerExtractionProcessor;

#[async_trait]
impl ResponseProcessor for FinalAnswerExtractionProcessor {
    fn kind(&self) -> ResponseProcessorKind {
        ResponseProcessorKind::FinalAnswerExtraction
    }

    async fn run(
        &self,
        ctx: &mut InvocationContext,
        response: &mut LlmResponseEnvelope,
        sink: &mut dyn EventSink,
    ) -> TalosResult<ResponseDisposition> {
        let has_tool_calls = response
            .output_items
            .iter()
            .any(|item| matches!(item, TalosOutputItem::ToolCall { .. }));
        if has_tool_calls {
            return Ok(ResponseDisposition::Continue);
        }
        let mut emitted = false;

        for item in &response.output_items {
            if let TalosOutputItem::OutputText { text } = item {
                sink.push(ExecutionEvent::final_response(
                    ctx,
                    response.branch,
                    text.clone(),
                ))
                .await?;
                emitted = true;
            }
        }

        if !emitted && !response.output_text.trim().is_empty() {
            sink.push(ExecutionEvent::final_response(
                ctx,
                response.branch,
                response.output_text.clone(),
            ))
            .await?;
            emitted = true;
        }

        if emitted {
            Ok(ResponseDisposition::Final)
        } else {
            Ok(ResponseDisposition::Continue)
        }
    }
}

pub struct EventProjectionProcessor;

#[async_trait]
impl ResponseProcessor for EventProjectionProcessor {
    fn kind(&self) -> ResponseProcessorKind {
        ResponseProcessorKind::EventProjection
    }

    async fn run(
        &self,
        ctx: &mut InvocationContext,
        response: &mut LlmResponseEnvelope,
        sink: &mut dyn EventSink,
    ) -> TalosResult<ResponseDisposition> {
        for item in &response.output_items {
            match item {
                TalosOutputItem::Reasoning { text } => {
                    sink.push(ExecutionEvent::text(
                        ctx,
                        EventAuthor::Agent,
                        response.branch,
                        ExecutionEventKind::Reasoning,
                        text.clone(),
                        true,
                    ))
                    .await?;
                }
                TalosOutputItem::ToolResult {
                    call_id,
                    content,
                    is_error,
                } => {
                    let payload = serde_json::to_value(content).unwrap_or_else(|_| {
                        serde_json::json!({
                            "content": content.iter().map(|entry| match entry {
                                TalosContentItem::Text { text } => serde_json::json!({"text": text}),
                                TalosContentItem::Json { value } => value.clone(),
                            }).collect::<Vec<_>>()
                        })
                    });
                    sink.push(ExecutionEvent::tool_result(
                        ctx,
                        response.branch,
                        call_id.clone(),
                        payload,
                        *is_error,
                    ))
                    .await?;
                }
                TalosOutputItem::OutputText { .. } | TalosOutputItem::ToolCall { .. } => {}
            }
        }

        Ok(ResponseDisposition::Continue)
    }
}

#[derive(Default)]
pub struct RequestProcessorPipeline {
    processors: Vec<Arc<dyn RequestProcessor>>,
}

impl RequestProcessorPipeline {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn default_single(planner: Arc<dyn Planner>) -> Self {
        Self::new()
            .with_processor(Arc::new(NoopRequestProcessor::new(
                RequestProcessorKind::Basic,
            )))
            .with_processor(Arc::new(NoopRequestProcessor::new(
                RequestProcessorKind::Instructions,
            )))
            .with_processor(Arc::new(NoopRequestProcessor::new(
                RequestProcessorKind::Contents,
            )))
            .with_processor(Arc::new(PlanningRequestProcessor::new(planner)))
            .with_processor(Arc::new(NoopRequestProcessor::new(
                RequestProcessorKind::ToolContext,
            )))
    }

    pub fn default_auto(planner: Arc<dyn Planner>) -> Self {
        Self::default_single(planner).with_processor(Arc::new(NoopRequestProcessor::new(
            RequestProcessorKind::AgentTransfer,
        )))
    }

    pub fn with_processor(mut self, processor: Arc<dyn RequestProcessor>) -> Self {
        self.processors.push(processor);
        self
    }

    pub async fn run(
        &self,
        ctx: &mut InvocationContext,
        request: &mut LlmRequestEnvelope,
        sink: &mut dyn EventSink,
    ) -> TalosResult<()> {
        for processor in &self.processors {
            processor.run(ctx, request, sink).await?;
        }
        Ok(())
    }
}

#[derive(Default)]
pub struct ResponseProcessorPipeline {
    processors: Vec<Arc<dyn ResponseProcessor>>,
}

impl ResponseProcessorPipeline {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn default_single(planner: Arc<dyn Planner>) -> Self {
        Self::new()
            .with_processor(Arc::new(PlanningResponseProcessor::new(planner)))
            .with_processor(Arc::new(ToolCallExtractionProcessor))
            .with_processor(Arc::new(FinalAnswerExtractionProcessor))
            .with_processor(Arc::new(NoopResponseProcessor::new(
                ResponseProcessorKind::StateDelta,
            )))
            .with_processor(Arc::new(EventProjectionProcessor))
    }

    pub fn default_auto(planner: Arc<dyn Planner>) -> Self {
        Self::default_single(planner)
    }

    pub fn with_processor(mut self, processor: Arc<dyn ResponseProcessor>) -> Self {
        self.processors.push(processor);
        self
    }

    pub async fn run(
        &self,
        ctx: &mut InvocationContext,
        response: &mut LlmResponseEnvelope,
        sink: &mut dyn EventSink,
    ) -> TalosResult<ResponseDisposition> {
        let mut disposition = ResponseDisposition::Continue;
        for processor in &self.processors {
            let current = processor.run(ctx, response, sink).await?;
            if matches!(current, ResponseDisposition::Final) {
                disposition = ResponseDisposition::Final;
            }
        }
        Ok(disposition)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ContextStateBags, ExecutionBranch, InvocationLimits, PlanningResponseEffect,
        PlannerKind, TurnPhase, VecEventSink,
    };
    use async_trait::async_trait;
    use serde_json::json;
    use std::collections::HashMap;

    struct BranchingPlanner;

    #[async_trait]
    impl Planner for BranchingPlanner {
        fn kind(&self) -> crate::PlannerKind {
            PlannerKind::PlanReact
        }

        fn process_planning_response_effect(
            &self,
            _ctx: &InvocationContext,
            response_text: &str,
        ) -> PlanningResponseEffect {
            PlanningResponseEffect {
                normalized_text: Some(response_text.trim().to_string()),
                state_delta: vec![crate::StateDelta {
                    namespace: crate::StateNamespace::Temp,
                    key: "plan_seen".to_string(),
                    value: json!(true),
                }],
                branch_override: Some(ExecutionBranch::DirectAnswer),
            }
        }

        async fn plan(&self, _ctx: &InvocationContext) -> TalosResult<crate::TaskPlan> {
            Ok(crate::TaskPlan::default())
        }
    }

    fn mock_ctx() -> InvocationContext {
        InvocationContext {
            session_id: "s1".to_string(),
            invocation_id: "i1".to_string(),
            agent_id: "a1".to_string(),
            workspace_id: "w1".to_string(),
            input: "hello".to_string(),
            model: "m1".to_string(),
            phase: TurnPhase::Execution,
            planning_instruction: None,
            transfer_to: None,
            execution_state: crate::InvocationExecutionState::default(),
            session: crate::InvocationSession {
                session_id: "s1".to_string(),
                ..crate::InvocationSession::default()
            },
            state: HashMap::new(),
            state_bags: ContextStateBags::default(),
            limits: InvocationLimits::default(),
            events: Vec::new(),
            stream_tx: None,
        }
    }

    #[tokio::test]
    async fn test_planning_response_processor_emits_typed_branch_override_event() {
        let planner: Arc<dyn Planner> = Arc::new(BranchingPlanner);
        let processor = PlanningResponseProcessor::new(planner);
        let mut ctx = mock_ctx();
        let mut response = LlmResponseEnvelope {
            output_text: "  answer ".to_string(),
            output_items: Vec::new(),
            branch: ExecutionBranch::ReactiveLoop,
        };
        let mut sink = VecEventSink::default();

        let disposition = processor.run(&mut ctx, &mut response, &mut sink).await.unwrap();
        let events = sink.into_events();

        assert_eq!(disposition, ResponseDisposition::Continue);
        assert_eq!(ctx.execution_state.planning_response.as_deref(), Some("answer"));
        assert!(!ctx.state_bags.request_state.contains_key("execution_branch"));
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].branch, ExecutionBranch::DirectAnswer);
        assert_eq!(
            events[0].actions.branch_override,
            Some(ExecutionBranch::DirectAnswer)
        );
        assert_eq!(events[0].actions.state_delta.len(), 1);
    }

    #[tokio::test]
    async fn test_final_answer_extraction_emits_final_event_and_marks_final() {
        let mut ctx = mock_ctx();
        let mut response = LlmResponseEnvelope {
            output_text: "done".to_string(),
            output_items: vec![TalosOutputItem::OutputText {
                text: "done".to_string(),
            }],
            branch: ExecutionBranch::DirectAnswer,
        };
        let mut sink = VecEventSink::default();

        let disposition = FinalAnswerExtractionProcessor
            .run(&mut ctx, &mut response, &mut sink)
            .await
            .unwrap();
        let events = sink.into_events();

        assert_eq!(disposition, ResponseDisposition::Final);
        assert_eq!(events.len(), 1);
        assert!(events[0].is_final_response());
        assert_eq!(events[0].branch, ExecutionBranch::DirectAnswer);
    }

    #[tokio::test]
    async fn test_tool_call_extraction_emits_tool_call_events() {
        let mut ctx = mock_ctx();
        let mut response = LlmResponseEnvelope {
            output_text: String::new(),
            output_items: vec![TalosOutputItem::ToolCall {
                call_id: "call-1".to_string(),
                name: "read_file".to_string(),
                arguments: json!({"path": "README.md"}),
            }],
            branch: ExecutionBranch::ReactiveLoop,
        };
        let mut sink = VecEventSink::default();

        let disposition = ToolCallExtractionProcessor
            .run(&mut ctx, &mut response, &mut sink)
            .await
            .unwrap();
        let events = sink.into_events();

        assert_eq!(disposition, ResponseDisposition::Continue);
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0].kind, ExecutionEventKind::ToolCall));
        assert_eq!(events[0].branch, ExecutionBranch::ReactiveLoop);
    }
}
