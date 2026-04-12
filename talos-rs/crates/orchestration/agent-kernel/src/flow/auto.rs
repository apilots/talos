use super::*;

pub struct AutoFlow {
    inner: SingleFlow,
}

impl AutoFlow {
    pub fn new(mut inner: SingleFlow) -> Self {
        inner.allow_transfer = true;
        inner.rebuild_pipelines();
        Self { inner }
    }

    pub fn inner(&self) -> &SingleFlow {
        &self.inner
    }
}

#[async_trait]
impl BaseLlmFlow for AutoFlow {
    fn settings(&self) -> &KernelExecutionSettings {
        self.inner.settings()
    }

    fn profile(&self) -> FlowProfile {
        self.inner.profile()
    }

    async fn run_events(
        &self,
        ctx: &mut InvocationContext,
        input: &str,
    ) -> TalosResult<FlowExecution> {
        self.inner.run_events(ctx, input).await
    }
}
