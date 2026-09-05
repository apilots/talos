use talos_agent::{
    AgentContext, AgentError, AgentErrorKind, AgentResult, SessionTurnInputDelivery,
    SessionTurnParams, SessionTurnResult, TurnFileScope,
};
use talos_agent_api::{AgentPendingInput, AgentStopReason};
use talos_agent_kernel::{
    build_active_turn_input, ActiveTurnInputDelivery, AgentKernelContinuation,
    AgentKernelExecution, AgentKernelIo, AgentKernelPendingInput, AgentKernelRequest,
    AgentKernelResult, GeneralTurnSkillOptions,
};
use talos_memory::{MemoryScope, UserWorkspaceMemorySnapshot};
use talos_prompt::{UserWorkspaceMemoryPromptContext, UserWorkspaceMemoryPromptFileState};
use talos_types::{PendingUserAction, UserActionState};
use tokio::sync::{mpsc, oneshot};

use crate::continuation::{build_kernel_continuation, KernelProviderContinuation};
use crate::event_mapper::KernelAgentEventMapper;
use crate::input::{project_input, KernelInputProjection};
use crate::{KernelAgentProvider, KernelAgentSession};

pub(crate) async fn execute(
    session: &KernelAgentSession,
    mut turn: SessionTurnParams,
) -> AgentResult<SessionTurnResult> {
    let provider = &session.provider;
    provider.validate_agent_id(&turn.context.session.agent_id)?;
    let projection = project_input(&turn.initial_input)?;
    let kernel_config = provider.effective_kernel_config(&turn.context);
    let continuation_reference = turn
        .continuation
        .as_ref()
        .map(KernelProviderContinuation::decode)
        .transpose()?;
    let continuation = prepare_continuation(
        provider,
        &turn,
        &projection,
        &kernel_config.workspace_dir,
        continuation_reference.as_ref(),
    )
    .await?;
    let mut mapper = match continuation_reference.as_ref() {
        Some(reference) => KernelAgentEventMapper::resumed(
            turn.events.clone(),
            &reference.message_id,
            &reference.block_id,
            &reference.call_id,
        )?,
        None => KernelAgentEventMapper::new(turn.events.clone()),
    };
    let result = execute_mapped(
        session,
        &mut turn,
        &mut mapper,
        projection,
        continuation,
        kernel_config,
    )
    .await;
    match result {
        Ok(completion) => Ok(completion),
        Err(error) => Err(mapper.settle_failure(error).await),
    }
}

async fn execute_mapped(
    session: &KernelAgentSession,
    turn: &mut SessionTurnParams,
    mapper: &mut KernelAgentEventMapper,
    projection: KernelInputProjection,
    continuation: Option<AgentKernelContinuation>,
    kernel_config: talos_config::AgentKernelConfig,
) -> AgentResult<SessionTurnResult> {
    let provider = &session.provider;
    let request = build_request(provider, turn, projection, continuation, kernel_config).await?;
    let (kernel_input_tx, kernel_input_rx) = mpsc::channel(32);
    let mut execution = session.kernel.execute(
        request,
        AgentKernelIo {
            input: kernel_input_rx,
            tasks: turn.control.tasks(),
        },
    )?;
    let mut input_open = true;
    let mut pending_input: Option<(SessionTurnInputDelivery, oneshot::Receiver<()>)> = None;

    loop {
        tokio::select! {
            event = execution.events.recv() => {
                let Some(event) = event else {
                    break;
                };
                mapper.map(event).await?;
            }
            delivery = turn.input.receive(), if input_open && pending_input.is_none() => {
                let Some(delivery) = delivery else {
                    input_open = false;
                    continue;
                };
                match prepare_live_input(provider, turn, delivery.input()).await {
                    Ok(input) => {
                        let (accepted_tx, accepted_rx) = oneshot::channel();
                        if kernel_input_tx
                            .send(ActiveTurnInputDelivery::new(input, accepted_tx))
                            .await
                            .is_err()
                        {
                            delivery.reject(AgentError::input_rejected(
                                "Kernel input receiver is closed",
                            ));
                            continue;
                        }
                        pending_input = Some((delivery, accepted_rx));
                    }
                    Err(error) => delivery.reject(error),
                }
            }
            accepted = async {
                (&mut pending_input
                    .as_mut()
                    .expect("pending input exists")
                    .1)
                    .await
            }, if pending_input.is_some() => {
                let (delivery, _) = pending_input.take().expect("pending input exists");
                match accepted {
                    Ok(()) => delivery.accept(),
                    Err(_) => delivery.reject(AgentError::input_rejected(
                        "Kernel stopped before accepting live input",
                    )),
                }
            }
        }
    }
    if let Some((delivery, accepted_rx)) = pending_input {
        match accepted_rx.await {
            Ok(()) => delivery.accept(),
            Err(_) => delivery.reject(AgentError::input_rejected(
                "Kernel stopped before accepting live input",
            )),
        }
    }
    drop(kernel_input_tx);
    let result = await_kernel_result(execution).await?;
    map_result(mapper, result).await
}

async fn build_request(
    provider: &KernelAgentProvider,
    invocation: &SessionTurnParams,
    projection: KernelInputProjection,
    continuation: Option<AgentKernelContinuation>,
    kernel_config: talos_config::AgentKernelConfig,
) -> AgentResult<AgentKernelRequest> {
    let context = &invocation.context;
    let resolved_attachments =
        resolve_attachments(provider, context, &projection.input.file_ids).await?;
    let user_workspace_memory = load_user_workspace_memory(provider, invocation).await?;
    let skill_roots = provider
        .skill_roots_resolver
        .skill_roots(&kernel_config)
        .map_err(AgentError::from)?;

    Ok(AgentKernelRequest {
        session_id: context.session.session_id.clone(),
        user_id: context.session.workspace.user_id.clone(),
        turn_id: context.turn_id.clone(),
        agent_id: context.session.agent_id.clone(),
        workspace_id: context.session.workspace.workspace_id.clone(),
        working_directory: context.working_directory.clone(),
        input: projection.input,
        resolved_attachments,
        model_selection: context.model_selection.clone(),
        model: context.model.clone(),
        environment: context.environment.clone(),
        kernel_config,
        continuation,
        skill_options: GeneralTurnSkillOptions {
            selected: projection.skills,
            roots: Some(skill_roots),
        },
        user_workspace_memory,
    })
}

async fn prepare_continuation(
    provider: &KernelAgentProvider,
    invocation: &SessionTurnParams,
    projection: &KernelInputProjection,
    workspace_root: &str,
    reference: Option<&KernelProviderContinuation>,
) -> AgentResult<Option<AgentKernelContinuation>> {
    let Some(reference) = reference else {
        if projection.user_action.is_some() {
            return Err(AgentError::new(
                AgentErrorKind::ContinuationInvalid,
                "UserAction response requires a Kernel continuation",
            ));
        }
        return Ok(None);
    };
    let (action_id, response) = projection.user_action.clone().ok_or_else(|| {
        AgentError::new(
            AgentErrorKind::ContinuationInvalid,
            "Kernel continuation requires one UserAction response",
        )
    })?;
    build_kernel_continuation(
        provider.session_artifact_store.as_ref(),
        std::path::Path::new(workspace_root),
        &invocation.context.session.session_id,
        &invocation.context.turn_id,
        reference,
        &action_id,
        response,
    )
    .await
    .map(Some)
}

async fn prepare_live_input(
    provider: &KernelAgentProvider,
    invocation: &SessionTurnParams,
    message: &talos_agent_api::AgentMessage,
) -> AgentResult<talos_agent_kernel::ActiveTurnInput> {
    let projection = project_input(message)?;
    if projection.user_action.is_some() || !projection.skills.is_empty() {
        return Err(AgentError::input_rejected(
            "live Kernel input supports only text and files",
        ));
    }
    let context = &invocation.context;
    let resolved = resolve_attachments(provider, context, &projection.input.file_ids).await?;
    let llm_provider = provider
        .llm_provider_router
        .provider_for(&context.model_selection.model_ref)
        .map_err(AgentError::from)?;
    build_active_turn_input(
        projection.input,
        resolved,
        &context.model,
        llm_provider.as_ref(),
    )
    .map_err(AgentError::from)
}

async fn resolve_attachments(
    provider: &KernelAgentProvider,
    context: &AgentContext,
    file_ids: &[String],
) -> AgentResult<Vec<talos_agent_kernel::ResolvedAttachment>> {
    let resolved = provider
        .attachment_reader
        .read(
            &TurnFileScope {
                owner_user_id: context.session.workspace.user_id.clone(),
                project_id: context.project_id.clone(),
            },
            file_ids,
        )
        .await
        .map_err(AgentError::from)?;
    Ok(resolved
        .into_iter()
        .map(|attachment| talos_agent_kernel::ResolvedAttachment {
            file_id: attachment.file_id,
            filename: attachment.filename,
            media_type: attachment.media_type,
            size_bytes: attachment.size_bytes,
            bytes: attachment.bytes,
        })
        .collect())
}

async fn load_user_workspace_memory(
    provider: &KernelAgentProvider,
    invocation: &SessionTurnParams,
) -> AgentResult<UserWorkspaceMemoryPromptContext> {
    let context = &invocation.context;
    let snapshot = provider
        .user_workspace_memory_reader
        .read(&MemoryScope {
            workspace_id: context.session.workspace.workspace_id.clone(),
            user_id: context.session.workspace.user_id.clone(),
            workspace_root: context.session.workspace.root.clone(),
        })
        .await
        .map_err(AgentError::from)?;
    let (file_state, version_token, content) = match snapshot {
        UserWorkspaceMemorySnapshot::Missing { template } => (
            UserWorkspaceMemoryPromptFileState::Missing,
            None,
            template.prompt_view().map_err(|error| {
                AgentError::provider_error("render missing User Workspace Memory")
                    .with_source(error.to_string())
            })?,
        ),
        UserWorkspaceMemorySnapshot::Valid {
            version_token,
            document,
            ..
        } => (
            UserWorkspaceMemoryPromptFileState::Valid,
            Some(version_token),
            document.prompt_view().map_err(|error| {
                AgentError::provider_error("render User Workspace Memory")
                    .with_source(error.to_string())
            })?,
        ),
        UserWorkspaceMemorySnapshot::Invalid {
            raw_content,
            version_token,
            ..
        } => (
            UserWorkspaceMemoryPromptFileState::Invalid,
            Some(version_token),
            raw_content,
        ),
    };
    Ok(UserWorkspaceMemoryPromptContext {
        file_state,
        version_token,
        content,
    })
}

async fn await_kernel_result(execution: AgentKernelExecution) -> AgentResult<AgentKernelResult> {
    execution
        .result
        .await
        .map_err(|_| AgentError::new(AgentErrorKind::BackendTaskLost, "Kernel task stopped"))?
        .map_err(AgentError::from)
}

async fn map_result(
    mapper: &mut KernelAgentEventMapper,
    result: AgentKernelResult,
) -> AgentResult<SessionTurnResult> {
    match result {
        AgentKernelResult::Completed { stop_reason, .. } => {
            mapper.finish_completed().await?;
            Ok(SessionTurnResult::Completed {
                stop_reason: map_stop_reason(stop_reason)?,
            })
        }
        AgentKernelResult::WaitingForInput {
            pending, metrics, ..
        } => match *pending {
            AgentKernelPendingInput::UserAction {
                action_id,
                call_id,
                tool_name,
                params,
            } => {
                let (message_id, block_id) = mapper.waiting_handles(&call_id)?;
                let message_id = message_id.to_string();
                let block_id = block_id.to_string();
                let continuation = KernelProviderContinuation::new(
                    action_id.clone(),
                    call_id.clone(),
                    tool_name,
                    message_id,
                    block_id.clone(),
                    metrics,
                )
                .encode()?;
                Ok(SessionTurnResult::WaitingForInput {
                    pending: Box::new(AgentPendingInput::UserAction {
                        action: PendingUserAction {
                            action_id,
                            call_id,
                            item_id: Some(block_id),
                            state: UserActionState::Pending,
                            params,
                        },
                    }),
                    continuation,
                })
            }
        },
    }
}

fn map_stop_reason(reason: talos_agent_kernel::StopReason) -> AgentResult<AgentStopReason> {
    match reason {
        talos_agent_kernel::StopReason::EndTurn => Ok(AgentStopReason::EndTurn),
        talos_agent_kernel::StopReason::ModelOutputLimit => Ok(AgentStopReason::OutputLimit),
        talos_agent_kernel::StopReason::TransferUnresolved => {
            Ok(AgentStopReason::TransferUnresolved)
        }
        talos_agent_kernel::StopReason::WaitingUserAction => Err(AgentError::provider_error(
            "Kernel completed result cannot use waiting stop reason",
        )),
    }
}
