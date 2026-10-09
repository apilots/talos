use std::path::PathBuf;

use agent_pb as pb;
use talos_agent_api::AgentRunMetrics;
use talos_config::{Config, ModelCostConfig, ProviderModelConfig};
use talos_error::TalosResult;
use talos_types::{
    talos::TalosSessionEnvironment, AgentTurnRef, ContextUsage, ModelInfo, ModelInputKind,
    ModelRef, ModelSelection, ModelSpeed, ReasoningEffort, TalosConnectionMode, TokenUsage,
    UserPermissionMode,
};

use super::invalid;
use crate::{AgentContext, AgentSessionContext};

pub fn encode_turn_ref(value: AgentTurnRef) -> pb::TurnRef {
    pb::TurnRef {
        session_id: value.session_id,
        turn_id: value.turn_id,
    }
}

pub fn decode_turn_ref(value: Option<pb::TurnRef>) -> TalosResult<AgentTurnRef> {
    let value = value.ok_or_else(invalid)?;
    if value.session_id.is_empty() || value.turn_id.is_empty() {
        return Err(invalid());
    }
    Ok(AgentTurnRef::new(value.session_id, value.turn_id))
}

pub fn encode_session_context(value: AgentSessionContext) -> pb::SessionContext {
    pb::SessionContext {
        agent_id: value.agent_id,
        session_id: value.session_id,
        user_id: value.workspace.user_id,
        workspace_id: value.workspace.workspace_id,
    }
}

pub fn decode_session_context(
    value: Option<pb::SessionContext>,
    config: &Config,
) -> TalosResult<AgentSessionContext> {
    let value = value.ok_or_else(invalid)?;
    if value.agent_id.is_empty() || value.session_id.is_empty() {
        return Err(invalid());
    }
    Ok(AgentSessionContext {
        agent_id: value.agent_id,
        session_id: value.session_id,
        workspace: config.workspaces.resolve(
            &config.talos_paths(),
            &value.workspace_id,
            &value.user_id,
        )?,
    })
}

pub fn encode_context(value: AgentContext) -> TalosResult<pb::AgentContext> {
    Ok(pb::AgentContext {
        session: Some(encode_session_context(value.session)),
        turn_id: value.turn_id,
        project_id: value.project_id,
        working_directory: value.working_directory.map(path_text).transpose()?,
        environment: value.environment.map(encode_environment).transpose()?,
        model: Some(encode_model(value.model)),
        model_selection: Some(encode_model_selection(value.model_selection)),
        permission_mode: encode_permission(value.permission_mode),
    })
}

pub fn decode_context(
    value: Option<pb::AgentContext>,
    config: &Config,
) -> TalosResult<AgentContext> {
    let value = value.ok_or_else(invalid)?;
    if value.turn_id.is_empty() {
        return Err(invalid());
    }
    Ok(AgentContext {
        session: decode_session_context(value.session, config)?,
        turn_id: value.turn_id,
        project_id: value.project_id,
        working_directory: value.working_directory.map(PathBuf::from),
        environment: value.environment.map(decode_environment).transpose()?,
        model: decode_model(value.model)?,
        model_selection: decode_model_selection(value.model_selection)?,
        permission_mode: decode_permission(value.permission_mode)?,
    })
}

pub fn encode_metrics(value: AgentRunMetrics) -> pb::RunMetrics {
    pb::RunMetrics {
        model_rounds: value.model_rounds,
        tool_calls: value.tool_calls,
        tool_successes: value.tool_successes,
        tool_errors: value.tool_errors,
        usage: Some(pb::TokenUsage {
            input_tokens: value.usage.input_tokens,
            output_tokens: value.usage.output_tokens,
            total_tokens: value.usage.total_tokens,
            cache_creation_input_tokens: value.usage.cache_creation_input_tokens,
            cache_read_input_tokens: value.usage.cache_read_input_tokens,
        }),
        context: value.context.map(|value| pb::ContextUsage {
            used_tokens: value.used_tokens,
            context_window_tokens: value.context_window_tokens,
            effective_prompt_budget_tokens: value.effective_prompt_budget_tokens,
            measured_at: value.measured_at,
        }),
        duration_ms: value.duration_ms,
    }
}

pub fn decode_metrics(value: Option<pb::RunMetrics>) -> TalosResult<AgentRunMetrics> {
    let value = value.ok_or_else(invalid)?;
    let usage = value.usage.ok_or_else(invalid)?;
    Ok(AgentRunMetrics {
        model_rounds: value.model_rounds,
        tool_calls: value.tool_calls,
        tool_successes: value.tool_successes,
        tool_errors: value.tool_errors,
        usage: TokenUsage {
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
            total_tokens: usage.total_tokens,
            cache_creation_input_tokens: usage.cache_creation_input_tokens,
            cache_read_input_tokens: usage.cache_read_input_tokens,
        },
        context: value.context.map(|value| ContextUsage {
            used_tokens: value.used_tokens,
            context_window_tokens: value.context_window_tokens,
            effective_prompt_budget_tokens: value.effective_prompt_budget_tokens,
            measured_at: value.measured_at,
        }),
        duration_ms: value.duration_ms,
    })
}

fn path_text(value: PathBuf) -> TalosResult<String> {
    value.into_os_string().into_string().map_err(|_| invalid())
}

fn encode_environment(value: TalosSessionEnvironment) -> TalosResult<pb::SessionEnvironment> {
    Ok(pb::SessionEnvironment {
        directory: path_text(value.directory)?,
        connection_mode: match value.connection_mode {
            TalosConnectionMode::Embedded => pb::ConnectionMode::Embedded as i32,
            TalosConnectionMode::Remote => pb::ConnectionMode::Remote as i32,
        },
    })
}

fn decode_environment(value: pb::SessionEnvironment) -> TalosResult<TalosSessionEnvironment> {
    let connection_mode = match pb::ConnectionMode::try_from(value.connection_mode) {
        Ok(pb::ConnectionMode::Embedded) => TalosConnectionMode::Embedded,
        Ok(pb::ConnectionMode::Remote) => TalosConnectionMode::Remote,
        _ => return Err(invalid()),
    };
    Ok(TalosSessionEnvironment {
        directory: PathBuf::from(value.directory),
        connection_mode,
    })
}

fn encode_model_selection(value: ModelSelection) -> pb::ModelSelection {
    pb::ModelSelection {
        provider: value.model_ref.provider,
        model_id: value.model_ref.id,
        reasoning_effort: value.reasoning_effort.map(encode_reasoning),
    }
}

fn decode_model_selection(value: Option<pb::ModelSelection>) -> TalosResult<ModelSelection> {
    let value = value.ok_or_else(invalid)?;
    if value.provider.is_empty() || value.model_id.is_empty() {
        return Err(invalid());
    }
    Ok(ModelSelection {
        model_ref: ModelRef {
            provider: value.provider,
            id: value.model_id,
        },
        reasoning_effort: value.reasoning_effort.map(decode_reasoning).transpose()?,
    })
}

fn encode_model(value: ModelInfo) -> pb::ModelInfo {
    let model = value.model;
    pb::ModelInfo {
        provider: value.provider,
        id: model.id,
        name: model.name,
        context_window: model.context_window,
        reasoning: model.reasoning,
        reasoning_effort: encode_reasoning(model.reasoning_effort),
        speed: match model.speed {
            ModelSpeed::Standard => pb::ModelSpeed::Standard as i32,
            ModelSpeed::Fast => pb::ModelSpeed::Fast as i32,
        },
        input: model.input.into_iter().map(encode_input_kind).collect(),
        cost: Some(pb::ModelCost {
            input: model.cost.input,
            output: model.cost.output,
            cache_read: model.cost.cache_read,
            cache_write: model.cost.cache_write,
        }),
    }
}

fn decode_model(value: Option<pb::ModelInfo>) -> TalosResult<ModelInfo> {
    let value = value.ok_or_else(invalid)?;
    if value.provider.is_empty() || value.id.is_empty() {
        return Err(invalid());
    }
    let speed = match pb::ModelSpeed::try_from(value.speed) {
        Ok(pb::ModelSpeed::Standard) => ModelSpeed::Standard,
        Ok(pb::ModelSpeed::Fast) => ModelSpeed::Fast,
        _ => return Err(invalid()),
    };
    let cost = value.cost.ok_or_else(invalid)?;
    Ok(ModelInfo::from_provider_model(
        value.provider,
        ProviderModelConfig {
            id: value.id,
            name: value.name,
            context_window: value.context_window,
            reasoning: value.reasoning,
            reasoning_effort: decode_reasoning(value.reasoning_effort)?,
            speed,
            input: value
                .input
                .into_iter()
                .map(decode_input_kind)
                .collect::<TalosResult<_>>()?,
            cost: ModelCostConfig {
                input: cost.input,
                output: cost.output,
                cache_read: cost.cache_read,
                cache_write: cost.cache_write,
            },
        },
    ))
}

fn encode_reasoning(value: ReasoningEffort) -> i32 {
    match value {
        ReasoningEffort::Light => pb::ReasoningEffort::Light as i32,
        ReasoningEffort::Medium => pb::ReasoningEffort::Medium as i32,
        ReasoningEffort::High => pb::ReasoningEffort::High as i32,
        ReasoningEffort::XHigh => pb::ReasoningEffort::Xhigh as i32,
    }
}

fn decode_reasoning(value: i32) -> TalosResult<ReasoningEffort> {
    match pb::ReasoningEffort::try_from(value) {
        Ok(pb::ReasoningEffort::Light) => Ok(ReasoningEffort::Light),
        Ok(pb::ReasoningEffort::Medium) => Ok(ReasoningEffort::Medium),
        Ok(pb::ReasoningEffort::High) => Ok(ReasoningEffort::High),
        Ok(pb::ReasoningEffort::Xhigh) => Ok(ReasoningEffort::XHigh),
        _ => Err(invalid()),
    }
}

fn encode_input_kind(value: ModelInputKind) -> i32 {
    match value {
        ModelInputKind::Text => pb::ModelInputKind::Text as i32,
        ModelInputKind::Image => pb::ModelInputKind::Image as i32,
        ModelInputKind::Audio => pb::ModelInputKind::Audio as i32,
        ModelInputKind::Video => pb::ModelInputKind::Video as i32,
        ModelInputKind::File => pb::ModelInputKind::File as i32,
    }
}

fn decode_input_kind(value: i32) -> TalosResult<ModelInputKind> {
    match pb::ModelInputKind::try_from(value) {
        Ok(pb::ModelInputKind::Text) => Ok(ModelInputKind::Text),
        Ok(pb::ModelInputKind::Image) => Ok(ModelInputKind::Image),
        Ok(pb::ModelInputKind::Audio) => Ok(ModelInputKind::Audio),
        Ok(pb::ModelInputKind::Video) => Ok(ModelInputKind::Video),
        Ok(pb::ModelInputKind::File) => Ok(ModelInputKind::File),
        _ => Err(invalid()),
    }
}

fn encode_permission(value: UserPermissionMode) -> i32 {
    match value {
        UserPermissionMode::AskForApproval => pb::PermissionMode::AskForApproval as i32,
        UserPermissionMode::ApproveForMe => pb::PermissionMode::ApproveForMe as i32,
        UserPermissionMode::FullAccess => pb::PermissionMode::FullAccess as i32,
    }
}

fn decode_permission(value: i32) -> TalosResult<UserPermissionMode> {
    match pb::PermissionMode::try_from(value) {
        Ok(pb::PermissionMode::AskForApproval) => Ok(UserPermissionMode::AskForApproval),
        Ok(pb::PermissionMode::ApproveForMe) => Ok(UserPermissionMode::ApproveForMe),
        Ok(pb::PermissionMode::FullAccess) => Ok(UserPermissionMode::FullAccess),
        _ => Err(invalid()),
    }
}

#[cfg(test)]
mod tests {
    use talos_config::{EffectiveWorkspace, TalosPaths};

    use super::*;

    #[test]
    fn context_projection_resolves_the_node_workspace_without_using_control_plane_root() {
        let node_home = PathBuf::from("/tmp/talos-remote-node");
        let config = Config::default().with_runtime_paths(TalosPaths::from_home(node_home.clone()));
        let context = AgentContext {
            session: AgentSessionContext {
                agent_id: "talos".into(),
                session_id: "session-1".into(),
                workspace: EffectiveWorkspace {
                    workspace_id: "default".into(),
                    user_id: "user-1".into(),
                    root: "/tmp/control-plane-workspace/user-1".into(),
                },
            },
            turn_id: "turn-1".into(),
            project_id: Some("project-1".into()),
            working_directory: Some("src".into()),
            environment: None,
            model: ModelInfo::from_provider_model(
                "openai",
                ProviderModelConfig {
                    id: "model-1".into(),
                    name: "Model".into(),
                    context_window: 128_000,
                    reasoning: true,
                    reasoning_effort: ReasoningEffort::High,
                    speed: ModelSpeed::Fast,
                    input: vec![ModelInputKind::Text, ModelInputKind::Image],
                    cost: ModelCostConfig::default(),
                },
            ),
            model_selection: ModelSelection::new(
                ModelRef {
                    provider: "openai".into(),
                    id: "model-1".into(),
                },
                Some(ReasoningEffort::High),
            ),
            permission_mode: UserPermissionMode::AskForApproval,
        };

        let decoded =
            decode_context(Some(encode_context(context.clone()).unwrap()), &config).unwrap();
        assert_eq!(
            decoded.session.workspace.root,
            node_home.join("workspace-talos/user-1")
        );
        assert_eq!(
            decoded.session.workspace.user_id,
            context.session.workspace.user_id
        );
        assert_eq!(decoded.model_selection, context.model_selection);
        assert_eq!(decoded.permission_mode, context.permission_mode);
    }
}
