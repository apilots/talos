use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum RequestProcessorKind {
    Basic = 0,
    Auth = 1,
    RequestConfirmation = 2,
    DynamicToolReplay = 3,
    Instructions = 4,
    Identity = 5,
    Compaction = 6,
    Contents = 7,
    ContextCache = 8,
    Interactions = 9,
    OutputSchema = 10,
    Planning = 11,
    ToolContext = 12,
    AgentTransfer = 13,
}

pub struct BasicRequestProcessor;

pub struct InstructionsRequestProcessor;
pub struct AuthRequestProcessor;
pub struct RequestConfirmationRequestProcessor;
pub struct DynamicToolReplayRequestProcessor;
pub struct ContextCacheRequestProcessor;
pub struct InteractionsRequestProcessor;
pub struct OutputSchemaRequestProcessor;

pub struct ToolContextRequestProcessor;

pub struct AgentTransferRequestProcessor;

#[async_trait]
impl RequestProcessor for InstructionsRequestProcessor {
    fn kind(&self) -> RequestProcessorKind {
        RequestProcessorKind::Instructions
    }

    async fn run(
        &self,
        _ctx: &mut InvocationContext,
        request: &mut LlmRequestEnvelope,
    ) -> TalosResult<RequestProcessorOutput> {
        if request.prompt_prefix_messages.is_empty() {
            if let Some(instruction) = &request.planning_instruction {
                request
                    .prompt_prefix_messages
                    .push(talos_llm_provider::ChatMessage::system(instruction.clone()));
            }
        }
        Ok(RequestProcessorOutput::default())
    }
}

#[async_trait]
impl RequestProcessor for BasicRequestProcessor {
    fn kind(&self) -> RequestProcessorKind {
        RequestProcessorKind::Basic
    }

    async fn run(
        &self,
        _ctx: &mut InvocationContext,
        request: &mut LlmRequestEnvelope,
    ) -> TalosResult<RequestProcessorOutput> {
        request.parallel_tool_calls =
            request.parallel_tool_calls && !request.tool_definitions.is_empty();
        Ok(RequestProcessorOutput::default())
    }
}

#[async_trait]
impl RequestProcessor for AuthRequestProcessor {
    fn kind(&self) -> RequestProcessorKind {
        RequestProcessorKind::Auth
    }

    async fn run(
        &self,
        ctx: &mut InvocationContext,
        request: &mut LlmRequestEnvelope,
    ) -> TalosResult<RequestProcessorOutput> {
        let latest_auth = latest_branch_event(ctx, request.branch, |event| {
            matches!(event.payload, KernelTranscriptPayload::Auth { .. })
        });
        let latest_user = latest_branch_event(ctx, request.branch, |event| {
            matches!(event.payload, KernelTranscriptPayload::Message { .. })
                && matches!(event.author, EventAuthor::User)
        });
        if let Some(event) = latest_auth {
            let auth_is_pending = latest_user
                .map(|user_event| user_event.timestamp <= event.timestamp)
                .unwrap_or(true);
            if auth_is_pending {
                if let KernelTranscriptPayload::Auth { payload } = &event.payload {
                    request.prompt_prefix_messages.push(ChatMessage::system(format!(
                        "Authentication is still pending for provider `{}`. Only continue once the credential challenge has been resolved. Context: {}",
                        payload.provider, payload.prompt
                    )));
                }
            }
        }
        Ok(RequestProcessorOutput::default())
    }
}

#[async_trait]
impl RequestProcessor for RequestConfirmationRequestProcessor {
    fn kind(&self) -> RequestProcessorKind {
        RequestProcessorKind::RequestConfirmation
    }

    async fn run(
        &self,
        ctx: &mut InvocationContext,
        request: &mut LlmRequestEnvelope,
    ) -> TalosResult<RequestProcessorOutput> {
        if let Some((confirmation_event, approval_event)) =
            latest_confirmation_approval_pair(ctx, request.branch)
        {
            let KernelTranscriptPayload::Confirmation {
                payload: confirmation,
            } = &confirmation_event.payload
            else {
                return Ok(RequestProcessorOutput::default());
            };
            let KernelTranscriptPayload::ApprovalResponse { payload: approval } =
                &approval_event.payload
            else {
                return Ok(RequestProcessorOutput::default());
            };

            if approval.approved {
                request.replay_tool_invocations.push(ReplayToolInvocation {
                    call_id: confirmation.call_id.clone(),
                    tool_name: confirmation.tool_name.clone(),
                    arguments: confirmation.arguments.clone().into(),
                });
            } else {
                let reason = approval.reason.as_deref().unwrap_or("no reason given");
                request.prompt_prefix_messages.push(ChatMessage::system(format!(
                    "Tool `{}` (call `{}`) was rejected by the user: {}. Inform the user and continue.",
                    approval.tool_name, approval.call_id, reason
                )));
            }
            return Ok(RequestProcessorOutput::default());
        }

        let latest_confirmation = latest_branch_event(ctx, request.branch, |event| {
            matches!(event.payload, KernelTranscriptPayload::Confirmation { .. })
        })
        .or_else(|| {
            latest_event(ctx, |event| {
                matches!(event.payload, KernelTranscriptPayload::Confirmation { .. })
            })
        });

        let Some(confirmation_event) = latest_confirmation else {
            return Ok(RequestProcessorOutput::default());
        };

        let KernelTranscriptPayload::Confirmation {
            payload: confirmation,
        } = &confirmation_event.payload
        else {
            return Ok(RequestProcessorOutput::default());
        };

        request.prompt_prefix_messages.push(ChatMessage::system(format!(
            "A tool confirmation is pending for `{}` (call `{}`). Wait for explicit user approval before continuing. Prompt: {}",
            confirmation.tool_name, confirmation.call_id, confirmation.prompt
        )));

        Ok(RequestProcessorOutput::default())
    }
}

fn latest_confirmation_approval_pair<'a>(
    ctx: &'a InvocationContext,
    branch: crate::ExecutionBranch,
) -> Option<(&'a KernelTranscriptEvent, &'a KernelTranscriptEvent)> {
    let latest_branch_pair = ctx.session.events.iter().rev().find_map(|approval_event| {
        if approval_event.branch != branch {
            return None;
        }
        confirmation_for_approval(ctx, approval_event)
    });
    latest_branch_pair.or_else(|| {
        ctx.session
            .events
            .iter()
            .rev()
            .find_map(|approval_event| confirmation_for_approval(ctx, approval_event))
    })
}

fn confirmation_for_approval<'a>(
    ctx: &'a InvocationContext,
    approval_event: &'a KernelTranscriptEvent,
) -> Option<(&'a KernelTranscriptEvent, &'a KernelTranscriptEvent)> {
    let KernelTranscriptPayload::ApprovalResponse { payload: approval } = &approval_event.payload
    else {
        return None;
    };
    ctx.session
        .events
        .iter()
        .rev()
        .find(|candidate| {
            if candidate.timestamp >= approval_event.timestamp {
                return false;
            }
            if let KernelTranscriptPayload::Confirmation {
                payload: confirmation,
            } = &candidate.payload
            {
                confirmation.call_id == approval.call_id
                    && confirmation.tool_name == approval.tool_name
            } else {
                false
            }
        })
        .map(|confirmation_event| (confirmation_event, approval_event))
}

#[async_trait]
impl RequestProcessor for DynamicToolReplayRequestProcessor {
    fn kind(&self) -> RequestProcessorKind {
        RequestProcessorKind::DynamicToolReplay
    }

    async fn run(
        &self,
        ctx: &mut InvocationContext,
        request: &mut LlmRequestEnvelope,
    ) -> TalosResult<RequestProcessorOutput> {
        let latest_dynamic_request = latest_branch_event(ctx, request.branch, |event| {
            matches!(
                event.payload,
                KernelTranscriptPayload::DynamicToolRequest { .. }
            )
        });

        let Some(dynamic_request_event) = latest_dynamic_request else {
            return Ok(RequestProcessorOutput::default());
        };

        let KernelTranscriptPayload::DynamicToolRequest {
            payload: dynamic_request,
        } = &dynamic_request_event.payload
        else {
            return Ok(RequestProcessorOutput::default());
        };

        let latest_dynamic_response = latest_branch_event(ctx, request.branch, |event| {
            if let KernelTranscriptPayload::DynamicToolResponse { payload } = &event.payload {
                payload.call_id == dynamic_request.call_id
            } else {
                false
            }
        });

        match latest_dynamic_response {
            Some(response_event) if response_event.timestamp > dynamic_request_event.timestamp => {
                let KernelTranscriptPayload::DynamicToolResponse { payload } =
                    &response_event.payload
                else {
                    return Ok(RequestProcessorOutput::default());
                };
                request.replay_tool_invocations.push(ReplayToolInvocation {
                    call_id: payload.call_id.clone(),
                    tool_name: payload.tool_name.clone(),
                    arguments: dynamic_request.arguments.clone().into(),
                });
                request.replay_dynamic_tool_results.insert(
                    payload.call_id.clone(),
                    ReplayDynamicToolResult {
                        output: payload.output.clone(),
                        success: payload.success,
                    },
                );
            }
            _ => {
                let timeout_ms = ctx.limits.dynamic_tool_timeout_ms;
                let elapsed_ms = (Utc::now() - dynamic_request_event.timestamp)
                    .num_milliseconds()
                    .max(0) as u64;
                if timeout_ms > 0 && elapsed_ms >= timeout_ms {
                    let timeout_message = format!(
                        "dynamic tool `{}` (call `{}`) timed out after {} ms",
                        dynamic_request.tool_name, dynamic_request.call_id, timeout_ms
                    );
                    match ctx.limits.dynamic_tool_timeout_behavior {
                        talos_config::DynamicToolTimeoutBehavior::ErrorToModel => {
                            request.replay_tool_invocations.push(ReplayToolInvocation {
                                call_id: dynamic_request.call_id.clone(),
                                tool_name: dynamic_request.tool_name.clone(),
                                arguments: dynamic_request.arguments.clone().into(),
                            });
                            request.replay_dynamic_tool_results.insert(
                                dynamic_request.call_id.clone(),
                                ReplayDynamicToolResult {
                                    output: timeout_message,
                                    success: false,
                                },
                            );
                        }
                        talos_config::DynamicToolTimeoutBehavior::AbortTurn => {
                            request.prompt_prefix_messages.push(ChatMessage::system(format!(
                                "Stop the turn and report that dynamic tool `{}` (call `{}`) timed out.",
                                dynamic_request.tool_name, dynamic_request.call_id
                            )));
                        }
                    }
                } else {
                    request.prompt_prefix_messages.push(ChatMessage::system(format!(
                        "A dynamic tool response is pending for `{}` (call `{}`). Wait for the external tool result before continuing.",
                        dynamic_request.tool_name, dynamic_request.call_id
                    )));
                }
            }
        }

        Ok(RequestProcessorOutput::default())
    }
}

#[async_trait]
impl RequestProcessor for ContextCacheRequestProcessor {
    fn kind(&self) -> RequestProcessorKind {
        RequestProcessorKind::ContextCache
    }

    async fn run(
        &self,
        ctx: &mut InvocationContext,
        request: &mut LlmRequestEnvelope,
    ) -> TalosResult<RequestProcessorOutput> {
        let visible_messages = ContentsBuilder::build_messages(
            &ctx.session,
            &ctx.invocation_id,
            request.branch,
            request.contents_mode,
        );
        if let Some(checkpoint) = ctx
            .session
            .events
            .iter()
            .rev()
            .find(|event| event.branch == request.branch && !event.partial)
        {
            request.cache_context = Some(crate::RequestCacheContext {
                checkpoint_invocation_id: checkpoint.invocation_id.clone(),
                visible_message_count: visible_messages.len(),
            });
        }
        Ok(RequestProcessorOutput::default())
    }
}

#[async_trait]
impl RequestProcessor for InteractionsRequestProcessor {
    fn kind(&self) -> RequestProcessorKind {
        RequestProcessorKind::Interactions
    }

    async fn run(
        &self,
        ctx: &mut InvocationContext,
        request: &mut LlmRequestEnvelope,
    ) -> TalosResult<RequestProcessorOutput> {
        if let Some(previous_agent_event) = ctx.session.events.iter().rev().find(|event| {
            event.branch == request.branch
                && !event.partial
                && matches!(event.author, EventAuthor::Agent)
                && matches!(event.payload, KernelTranscriptPayload::Message { .. })
        }) {
            request.interaction_context = Some(crate::RequestInteractionContext {
                previous_invocation_id: previous_agent_event.invocation_id.clone(),
            });
        }
        Ok(RequestProcessorOutput::default())
    }
}

#[async_trait]
impl RequestProcessor for OutputSchemaRequestProcessor {
    fn kind(&self) -> RequestProcessorKind {
        RequestProcessorKind::OutputSchema
    }

    async fn run(
        &self,
        _ctx: &mut InvocationContext,
        request: &mut LlmRequestEnvelope,
    ) -> TalosResult<RequestProcessorOutput> {
        if request.tool_definitions.is_empty() {
            return Ok(RequestProcessorOutput::default());
        }
        request.output_contract = RequestOutputContract::ToolMediated;
        request.prompt_prefix_messages.push(ChatMessage::system(
            "When tools are required, finish the tool work first and then return a final answer that is explicit and complete."
                .to_string(),
        ));
        Ok(RequestProcessorOutput::default())
    }
}
#[async_trait]
impl RequestProcessor for ToolContextRequestProcessor {
    fn kind(&self) -> RequestProcessorKind {
        RequestProcessorKind::ToolContext
    }

    async fn run(
        &self,
        _ctx: &mut InvocationContext,
        request: &mut LlmRequestEnvelope,
    ) -> TalosResult<RequestProcessorOutput> {
        if matches!(request.branch, crate::ExecutionBranch::DirectAnswer) {
            request.tool_definitions.clear();
            request.parallel_tool_calls = false;
            return Ok(RequestProcessorOutput::default());
        }
        if matches!(request.contents_mode, ContentsMode::CurrentInvocation) {
            request.parallel_tool_calls =
                request.parallel_tool_calls && !request.tool_definitions.is_empty();
        }
        Ok(RequestProcessorOutput::default())
    }
}

#[async_trait]
impl RequestProcessor for AgentTransferRequestProcessor {
    fn kind(&self) -> RequestProcessorKind {
        RequestProcessorKind::AgentTransfer
    }

    async fn run(
        &self,
        ctx: &mut InvocationContext,
        request: &mut LlmRequestEnvelope,
    ) -> TalosResult<RequestProcessorOutput> {
        if let Some(transfer_to) = ctx.transfer_to.clone() {
            if transfer_to.trim().is_empty() {
                ctx.transfer_to = None;
                return Ok(RequestProcessorOutput::default());
            }

            request.prompt_prefix_messages.push(talos_llm_provider::ChatMessage::system(
                format!(
                    "Route this turn to agent `{}` when handoff is required. Preserve tool context until transfer is resolved.",
                    transfer_to
                ),
            ));

            let mut transfer_event = ExecutionEvent::new(
                ctx,
                EventAuthor::System,
                request.branch,
                ExecutionEventKind::Planning,
                EventPayload::Empty,
                false,
            );
            transfer_event.actions.transfer_to = Some(transfer_to);
            return Ok(RequestProcessorOutput {
                events: vec![transfer_event],
            });
        }
        Ok(RequestProcessorOutput::default())
    }
}
