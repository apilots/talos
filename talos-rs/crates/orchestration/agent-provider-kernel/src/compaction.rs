use talos_agent::{
    AgentError, AgentOperationControl, AgentResult, SessionCompactionParams,
    SessionCompactionResult,
};
use talos_agent_kernel::{
    compact_session_memory, AgentKernelEvent, AgentKernelEventPayload, ContextCompactionStreamData,
    EventAuthor, ExecutionBranch, KernelTranscriptEvent, KernelTranscriptPayload,
    MemoryCompactionPolicy, MemoryFormationParams, SessionCompactionState, SessionStateUpdate,
};
use talos_memory::MemoryScope;
use talos_types::{
    ContextCompactionMetrics, ContextCompactionOutcome, ContextCompactionStage,
    ContextCompactionTrigger, TokenUsage,
};

use crate::event_mapper::KernelAgentEventMapper;
use crate::{KernelAgentProvider, KernelAgentSession};

pub(crate) async fn compact_session(
    session: &KernelAgentSession,
    params: SessionCompactionParams,
) -> AgentResult<SessionCompactionResult> {
    let _provider = &session.provider;
    let SessionCompactionParams {
        context,
        events,
        control,
    } = params;
    let mut mapper = KernelAgentEventMapper::new(events);
    let result = compact_mapped(session, &context, control, &mut mapper).await;
    match result {
        Ok(usage) => Ok(SessionCompactionResult { usage }),
        Err(error) => Err(mapper.settle_failure(error).await),
    }
}

async fn compact_mapped(
    session: &KernelAgentSession,
    context: &talos_agent::AgentContext,
    control: AgentOperationControl,
    mapper: &mut KernelAgentEventMapper,
) -> AgentResult<TokenUsage> {
    let provider = &session.provider;
    let config = provider.effective_kernel_config(context);
    let llm_provider = provider
        .llm_provider_router
        .provider_for(&context.model_selection.model_ref)
        .map_err(AgentError::from)?;
    let memory_scope = MemoryScope {
        workspace_id: context.session.workspace.workspace_id.clone(),
        user_id: context.session.workspace.user_id.clone(),
        workspace_root: context.session.workspace.root.clone(),
    };
    mapper
        .map(AgentKernelEvent::new(
            AgentKernelEventPayload::ContextCompaction(ContextCompactionStreamData {
                trigger: ContextCompactionTrigger::UserRequested,
                stage: ContextCompactionStage::Preparing,
                outcome: None,
                metrics: None,
                message: Some("User-requested context compaction started.".to_string()),
            }),
        ))
        .await?;

    let report = tokio::select! {
        result = compact_session_memory(
            MemoryFormationParams {
                session_id: &context.session.session_id,
                producer_agent_id: &provider.agent_id,
                dream_memory_store: provider.dream_memory_store.as_ref(),
                memory_scope: &memory_scope,
                session_artifact_store: provider.session_artifact_store.as_ref(),
                session_prompt_context_source: session.prompt_context.as_ref(),
                memory_progress_store: provider.memory_progress_store.as_ref(),
                kernel_config: &config,
            },
            &context.model.model.id,
            llm_provider,
            MemoryCompactionPolicy::for_compaction(),
        ) => result.map_err(AgentError::from)?,
        () = control.cancellation().requested() => {
            return Err(AgentError::new(
                talos_error::ErrorCode::Cancelled,
                "Kernel context compaction was cancelled",
            ));
        }
    };

    if report.covers_until_seq.is_some() {
        persist_recovery_state(provider, context, &config).await?;
    }
    let usage = TokenUsage {
        input_tokens: report.input_tokens,
        output_tokens: report.output_tokens,
        total_tokens: report.total_tokens,
        cache_creation_input_tokens: report.cache_creation_input_tokens,
        cache_read_input_tokens: report.cache_read_input_tokens,
    };
    let outcome = if report.covers_until_seq.is_some() {
        ContextCompactionOutcome::Applied
    } else {
        ContextCompactionOutcome::NoChanges
    };
    mapper
        .map(AgentKernelEvent::new(
            AgentKernelEventPayload::ContextCompaction(ContextCompactionStreamData {
                trigger: ContextCompactionTrigger::UserRequested,
                stage: ContextCompactionStage::Completed,
                outcome: Some(outcome),
                metrics: Some(ContextCompactionMetrics {
                    covers_from_seq: report.covers_from_seq,
                    covers_until_seq: report.covers_until_seq,
                    compaction_input_tokens: report.input_tokens,
                    compaction_output_tokens: report.output_tokens,
                    trimmed_context_unit_count: report.trimmed_context_unit_count,
                    ..ContextCompactionMetrics::default()
                }),
                message: Some("Session context compaction completed.".to_string()),
            }),
        ))
        .await?;
    mapper.finish_completed().await?;
    Ok(usage)
}

async fn persist_recovery_state(
    provider: &KernelAgentProvider,
    context: &talos_agent::AgentContext,
    config: &talos_config::AgentKernelConfig,
) -> AgentResult<()> {
    let event = KernelTranscriptEvent::new(
        &context.session.session_id,
        &context.turn_id,
        ExecutionBranch::ModelThinking,
        EventAuthor::System,
        KernelTranscriptPayload::SessionState {
            update: SessionStateUpdate::Compaction(SessionCompactionState::recovery()),
        },
    );
    provider
        .session_artifact_store
        .append_transcript_event(std::path::Path::new(&config.workspace_dir), &event)
        .await
        .map_err(AgentError::from)
}
