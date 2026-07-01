use super::*;

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
    ) -> TalosResult<RequestProcessorOutput> {
        let effect = self.planner.build_request_effect(ctx, request);
        request.reasoning_effort = effect.reasoning_effort;
        ctx.planning_instruction = effect.planning_instruction.clone();
        request.planning_instruction = effect.planning_instruction;
        Ok(RequestProcessorOutput::default())
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
    ) -> TalosResult<ResponseProcessorOutput> {
        let effect = self.planner.process_response_effect(ctx, response);

        if let Some(normalized) = effect.normalized_text {
            ctx.set_planning_response(normalized);
        }

        for segment in &effect.segments {
            match segment.kind {
                PlanningResponseSegmentKind::Thought => {
                    response.output_items.push(RunItem::Reasoning {
                        text: segment.text.clone(),
                    });
                }
                PlanningResponseSegmentKind::FinalAnswer => {}
            }
        }

        if let Some(output_text_override) = effect.output_text_override {
            response.output_text = output_text_override;
        } else if effect.clear_output_text {
            response.output_text.clear();
        }

        if !effect.state_delta.is_empty() || effect.branch_override.is_some() {
            if let Some(branch_override) = effect.branch_override {
                response.branch = branch_override;
            }
            response.actions = EventActions {
                state_delta: effect.state_delta.clone(),
                end_turn: false,
                transfer_to: None,
                branch_override: effect.branch_override,
                rewind_before_invocation_id: None,
                compaction: None,
            };
            let mut event = ExecutionEvent::new(
                ctx,
                EventAuthor::Agent,
                response.branch,
                ExecutionEventKind::Planning,
                EventPayload::Empty,
                false,
            );
            event.actions = response.actions.clone();
            return Ok(ResponseProcessorOutput {
                events: vec![event],
            });
        }

        Ok(ResponseProcessorOutput::default())
    }
}
