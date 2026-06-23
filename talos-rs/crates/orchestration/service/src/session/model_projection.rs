use talos_config::provider_config_for_llm;
use talos_error::TalosResult;
use talos_store::session::SessionRecord;
use talos_types::{
    SessionConf, SessionConfigOverridePolicy, SessionContext, SessionState, TalosConfig,
    TalosSession,
};

pub(crate) fn build_session(
    record: &SessionRecord,
    workspace_id: Option<String>,
    base_config: &TalosConfig,
) -> TalosResult<TalosSession> {
    let record = normalize_record_for_base_provider(record, base_config)?;
    let config =
        SessionConfigOverridePolicy::apply_to_config(base_config, &record.model, &record.conf)?;
    let mut session = TalosSession {
        title: record.title.clone(),
        user_id: record.user_id.clone(),
        context: SessionContext {
            session_id: record.id.clone(),
            workspace_id,
            source_type: record.context.source_type.clone(),
            provider: record.context.provider.clone(),
            user_id: record.context.user_id.clone(),
            message_id: record.context.message_id.clone(),
            trace_id: record.context.trace_id.clone(),
            channel_id: record.context.channel_id.clone(),
            thread_id: record.context.thread_id.clone(),
            project_id: record.project_id.clone(),
            current_directory: record.current_directory.clone(),
            project_root: record.context.project_root.clone(),
            environment: record.context.environment.clone(),
        },
        conf: record.conf.clone(),
        project_id: record.project_id.clone(),
        current_directory: record.current_directory.clone(),
        config,
        state: SessionState {
            created_at: record.created_at,
            last_activity: record.last_activity,
            message_count: record.message_count,
            usage: record.usage.clone(),
        },
    };
    session.context.project_root = None;
    Ok(session)
}

fn normalize_record_for_base_provider(
    record: &SessionRecord,
    base_config: &TalosConfig,
) -> TalosResult<SessionRecord> {
    let explicit_model = SessionConfigOverridePolicy::extract_agent_model(&record.conf)?;
    let effective_model = explicit_model.as_deref().unwrap_or(record.model.as_str());
    if base_provider_supports_model(base_config, effective_model) {
        return Ok(record.clone());
    }

    let mut normalized = record.clone();
    normalized.model = base_config.llm.model.clone();
    normalized.conf = clear_session_model_override(&normalized.conf);
    Ok(normalized)
}

fn base_provider_supports_model(base_config: &TalosConfig, model: &str) -> bool {
    if model == base_config.llm.model {
        return true;
    }
    let Some(provider) = provider_config_for_llm(&base_config.llm, &base_config.llm.provider)
    else {
        return false;
    };
    provider.models.is_empty() || provider.models.iter().any(|entry| entry.id == model)
}

fn clear_session_model_override(conf: &SessionConf) -> SessionConf {
    let mut next = conf.clone();
    if let Some(agent) = next.agent.as_mut() {
        agent.model = None;
    }
    if next
        .agent
        .as_ref()
        .is_some_and(|agent| agent.model.is_none())
    {
        next.agent = None;
    }
    next
}
