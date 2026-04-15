use super::*;

#[derive(Debug, Clone)]
pub struct FlowRunSummary {
    pub output_text: String,
    pub output_items: Vec<TalosOutputItem>,
    pub stop_reason: StopReason,
    pub usage: TalosUsage,
    pub warnings: Vec<KernelWarning>,
}

#[derive(Debug, Clone)]
pub struct FlowExecution {
    pub events: Vec<ExecutionEvent>,
    pub summary: FlowRunSummary,
}

#[async_trait]
pub trait BaseLlmFlow: Send + Sync {
    fn settings(&self) -> &KernelExecutionSettings;
    fn profile(&self) -> FlowProfile;
    async fn run_events(
        &self,
        ctx: &mut InvocationContext,
        input: &str,
    ) -> TalosResult<FlowExecution>;
}

impl From<crate::ExecutionPath> for ExecutionBranch {
    fn from(value: crate::ExecutionPath) -> Self {
        match value {
            crate::ExecutionPath::DirectAnswer => Self::DirectAnswer,
            crate::ExecutionPath::ReactiveLoop => Self::ReactiveLoop,
        }
    }
}
