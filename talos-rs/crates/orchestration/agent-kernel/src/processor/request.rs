use super::*;

#[async_trait]
pub trait RequestProcessor: Send + Sync {
    fn kind(&self) -> RequestProcessorKind;

    async fn run(
        &self,
        _ctx: &mut InvocationContext,
        _request: &mut LlmRequestEnvelope,
    ) -> TalosResult<RequestProcessorOutput> {
        Ok(RequestProcessorOutput::default())
    }
}

#[derive(Debug, Default, Clone)]
pub struct RequestProcessorOutput {
    pub events: Vec<ExecutionEvent>,
}

pub(crate) fn stage_request_event(ctx: &mut InvocationContext, event: &ExecutionEvent) {
    let staged = KernelTranscriptEvent::from_execution_event(ctx.session_id.clone(), event.clone());
    for delta in &staged.actions.state_delta {
        ctx.session.state.apply_delta(delta);
    }
    ctx.session.events.push(staged);
}

pub(crate) fn latest_branch_event<'a>(
    ctx: &'a InvocationContext,
    branch: crate::ExecutionBranch,
    predicate: impl Fn(&'a KernelTranscriptEvent) -> bool,
) -> Option<&'a KernelTranscriptEvent> {
    ctx.session
        .events
        .iter()
        .rev()
        .find(|event| event.branch == branch && predicate(event))
}

pub(crate) fn latest_event<'a>(
    ctx: &'a InvocationContext,
    predicate: impl Fn(&'a KernelTranscriptEvent) -> bool,
) -> Option<&'a KernelTranscriptEvent> {
    ctx.session
        .events
        .iter()
        .rev()
        .find(|event| predicate(event))
}

#[derive(Default)]
pub struct RequestProcessorPipeline {
    processors: Vec<Arc<dyn RequestProcessor>>,
}

impl RequestProcessorPipeline {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn for_profile(
        profile: FlowProfile,
        planner: Arc<dyn Planner>,
        agent_name: impl Into<String>,
        agent_description: impl Into<String>,
        allow_transfer: bool,
    ) -> Self {
        let agent_name = agent_name.into();
        let agent_description = agent_description.into();
        let mut pipeline = Self::new()
            .with_processor(Arc::new(BasicRequestProcessor))
            .with_processor(Arc::new(AuthRequestProcessor))
            .with_processor(Arc::new(RequestConfirmationRequestProcessor))
            .with_processor(Arc::new(DynamicToolReplayRequestProcessor))
            .with_processor(Arc::new(InstructionsRequestProcessor))
            .with_processor(Arc::new(IdentityRequestProcessor::new(
                agent_name,
                agent_description,
            )))
            .with_processor(Arc::new(CompactionRequestProcessor))
            .with_processor(Arc::new(ContentsRequestProcessor))
            .with_processor(Arc::new(ContextCacheRequestProcessor))
            .with_processor(Arc::new(InteractionsRequestProcessor));
        if matches!(
            profile,
            FlowProfile::DirectAnswer | FlowProfile::ModelThinking | FlowProfile::ReactiveLoop
        ) {
            pipeline = pipeline.with_processor(Arc::new(PlanningRequestProcessor::new(planner)));
        }
        pipeline = pipeline
            .with_processor(Arc::new(ToolContextRequestProcessor))
            .with_processor(Arc::new(OutputSchemaRequestProcessor));
        if allow_transfer {
            pipeline = pipeline.with_processor(Arc::new(AgentTransferRequestProcessor));
        }
        pipeline
    }

    pub fn with_processor(mut self, processor: Arc<dyn RequestProcessor>) -> Self {
        self.processors.push(processor);
        self
    }

    pub async fn run(
        &self,
        ctx: &mut InvocationContext,
        request: &mut LlmRequestEnvelope,
    ) -> TalosResult<Vec<ExecutionEvent>> {
        let mut events = Vec::new();
        for processor in &self.processors {
            let output = processor.run(ctx, request).await?;
            for event in &output.events {
                stage_request_event(ctx, event);
            }
            events.extend(output.events);
        }
        Ok(events)
    }
}
