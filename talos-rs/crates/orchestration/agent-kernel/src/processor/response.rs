use super::*;

#[async_trait]
pub trait ResponseProcessor: Send + Sync {
    fn kind(&self) -> ResponseProcessorKind;

    async fn run(
        &self,
        _ctx: &mut InvocationContext,
        _response: &mut LlmResponseEnvelope,
    ) -> TalosResult<ResponseProcessorOutput> {
        Ok(ResponseProcessorOutput::default())
    }
}

#[derive(Debug, Clone, Default)]
pub struct ResponseProcessorOutput {
    pub events: Vec<ExecutionEvent>,
}

#[derive(Default)]
pub struct ResponseProcessorPipeline {
    processors: Vec<Arc<dyn ResponseProcessor>>,
}

impl ResponseProcessorPipeline {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn for_profile(profile: FlowProfile, planner: Arc<dyn Planner>) -> Self {
        let mut pipeline = Self::new();
        if matches!(profile, FlowProfile::ReactiveLoop) {
            pipeline = pipeline.with_processor(Arc::new(PlanningResponseProcessor::new(planner)));
        }
        pipeline
            .with_processor(Arc::new(StateDeltaResponseProcessor))
            .with_processor(Arc::new(EventProjectionProcessor))
    }

    pub fn with_processor(mut self, processor: Arc<dyn ResponseProcessor>) -> Self {
        self.processors.push(processor);
        self
    }

    pub async fn run(
        &self,
        ctx: &mut InvocationContext,
        response: &mut LlmResponseEnvelope,
    ) -> TalosResult<ResponseProcessorOutput> {
        let mut events = Vec::new();
        for processor in &self.processors {
            let current = processor.run(ctx, response).await?;
            events.extend(current.events);
        }
        Ok(ResponseProcessorOutput { events })
    }
}
