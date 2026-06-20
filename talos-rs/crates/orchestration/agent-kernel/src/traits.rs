use crate::{
    InvocationContext, LlmRequestEnvelope, LlmResponseEnvelope, PlannerKind, PlanningRequestEffect,
    PlanningResponseEffect,
};
use async_trait::async_trait;

#[async_trait]
pub trait Planner: Send + Sync {
    fn mode(&self) -> PlannerKind {
        PlannerKind::BuiltIn
    }

    /// Produce the request-phase effect: planning instruction and optional reasoning effort.
    fn build_request_effect(
        &self,
        _ctx: &InvocationContext,
        _request: &LlmRequestEnvelope,
    ) -> PlanningRequestEffect {
        PlanningRequestEffect::default()
    }

    /// Produce the response-phase effect: normalized text, segments, final answer detection, etc.
    fn process_response_effect(
        &self,
        _ctx: &InvocationContext,
        _response: &LlmResponseEnvelope,
    ) -> PlanningResponseEffect {
        PlanningResponseEffect::default()
    }
}
