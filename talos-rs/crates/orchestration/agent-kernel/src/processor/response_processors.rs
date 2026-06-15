use super::*;

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
    ) -> TalosResult<ResponseProcessorOutput> {
        let mut events = Vec::new();
        for item in &response.output_items {
            match item {
                TalosOutputItem::Reasoning { text } => {
                    events.push(ExecutionEvent::text(
                        ctx,
                        EventAuthor::Agent,
                        response.branch,
                        ExecutionEventKind::Reasoning,
                        text.clone(),
                        true,
                    ));
                }
                TalosOutputItem::ExecutionSummary { text } => {
                    events.push(ExecutionEvent::text(
                        ctx,
                        EventAuthor::Agent,
                        response.branch,
                        ExecutionEventKind::Reasoning,
                        text.clone(),
                        true,
                    ));
                }
                TalosOutputItem::ToolResult {
                    call_id,
                    tool_name,
                    content,
                    is_error,
                } => {
                    let payload = serde_json::to_value(content).unwrap_or_else(|_| {
                        serde_json::json!({
                            "content": content.iter().map(TalosContentItem::display_text).collect::<Vec<_>>()
                        })
                    });
                    events.push(ExecutionEvent::tool_result(
                        ctx,
                        response.branch,
                        call_id.clone(),
                        tool_name.clone(),
                        payload,
                        *is_error,
                    ));
                }
                TalosOutputItem::OutputText { .. }
                | TalosOutputItem::ToolCall { .. }
                | TalosOutputItem::UserInput { .. } => {}
            }
        }

        Ok(ResponseProcessorOutput { events })
    }
}

pub struct StateDeltaResponseProcessor;

#[async_trait]
impl ResponseProcessor for StateDeltaResponseProcessor {
    fn kind(&self) -> ResponseProcessorKind {
        ResponseProcessorKind::StateDelta
    }

    async fn run(
        &self,
        ctx: &mut InvocationContext,
        response: &mut LlmResponseEnvelope,
    ) -> TalosResult<ResponseProcessorOutput> {
        for delta in &response.actions.state_delta {
            match delta.namespace {
                crate::StateNamespace::Session => {
                    ctx.set_session_state(delta.key.clone(), delta.value.clone());
                }
                crate::StateNamespace::App => {
                    ctx.state.insert(delta.key.clone(), delta.value.clone());
                }
                crate::StateNamespace::Temp => {
                    ctx.set_request_state(delta.key.clone(), delta.value.clone());
                }
                crate::StateNamespace::User | crate::StateNamespace::Workspace => {}
            }
        }
        Ok(ResponseProcessorOutput::default())
    }
}
