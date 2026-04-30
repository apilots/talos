use super::*;

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
    ) -> TalosResult<ResponseProcessorOutput> {
        let mut events = Vec::new();
        for item in &response.output_items {
            if let TalosOutputItem::ToolCall {
                call_id,
                name,
                arguments,
            } = item
            {
                events.push(ExecutionEvent::tool_call(
                    ctx,
                    response.branch,
                    call_id.clone(),
                    name.clone(),
                    arguments.clone().into(),
                ));
            }
        }

        Ok(ResponseProcessorOutput {
            disposition: ResponseDisposition::Continue,
            events,
        })
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
    ) -> TalosResult<ResponseProcessorOutput> {
        if !response.allow_final_projection {
            return Ok(ResponseProcessorOutput::default());
        }
        let has_tool_calls = response
            .output_items
            .iter()
            .any(|item| matches!(item, TalosOutputItem::ToolCall { .. }));
        if has_tool_calls {
            return Ok(ResponseProcessorOutput::default());
        }
        let mut emitted = false;
        let mut events = Vec::new();

        for item in &response.output_items {
            if let TalosOutputItem::OutputText { text } = item {
                events.push(ExecutionEvent::final_response(
                    ctx,
                    response.branch,
                    text.clone(),
                ));
                emitted = true;
            }
        }

        if !emitted && !response.output_text.trim().is_empty() {
            events.push(ExecutionEvent::final_response(
                ctx,
                response.branch,
                response.output_text.clone(),
            ));
            emitted = true;
        }

        if emitted {
            Ok(ResponseProcessorOutput {
                disposition: ResponseDisposition::Final,
                events,
            })
        } else {
            Ok(ResponseProcessorOutput::default())
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
                            "content": content.iter().map(|entry| match entry {
                                TalosContentItem::Text { text } => serde_json::json!({"text": text}),
                                TalosContentItem::Json { value } => value.clone().into(),
                            }).collect::<Vec<_>>()
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

        Ok(ResponseProcessorOutput {
            disposition: ResponseDisposition::Continue,
            events,
        })
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
