use std::sync::Arc;

use crate::session::{CreateSessionParams, SessionService};
use crate::{SessionHistoryService, TimelineEventService, TurnService};
use talos_error::{TalosError, TalosResult};
use talos_types::talos::{TalosContext, TalosInputItem, TalosProvider, TalosSourceType};
use talos_types::{
    AgentTurnRef, SpawnSubagentRequest, StartTurnRequest, SubagentRef, SubagentSpawnResult,
};

const MAX_SUBAGENT_PROMPT_BYTES: usize = 32 * 1024;
/// Owns Agent-tree Session orchestration.
///
/// This service validates ownership and composes existing Session, Turn, and
/// Timeline services. It must not own Agent registration, execution tasks,
/// status storage, event transport, or client rendering.
#[derive(Clone)]
pub(crate) struct SubagentService {
    pub(super) sessions: Arc<SessionService>,
    pub(super) turns: Arc<TurnService>,
    pub(super) timeline: Arc<TimelineEventService>,
    pub(super) history: SessionHistoryService,
}

impl SubagentService {
    pub(crate) fn new(
        sessions: Arc<SessionService>,
        turns: Arc<TurnService>,
        timeline: Arc<TimelineEventService>,
        history: SessionHistoryService,
    ) -> Self {
        Self {
            sessions,
            turns,
            timeline,
            history,
        }
    }

    pub(crate) async fn spawn(
        &self,
        request: SpawnSubagentRequest,
    ) -> TalosResult<SubagentSpawnResult> {
        Self::validate_request(&request)?;
        let caller = self.authorize_caller(&request.caller).await?;
        let agent_id = caller.agent_id.clone();
        self.turns.validate_subagent_agent(&agent_id).await?;
        let child_path = caller.path.join(&request.task_name)?;
        let parent = self
            .sessions
            .get_session(&request.caller.session_id)
            .await?
            .ok_or_else(|| {
                TalosError::ResourceNotFound(format!(
                    "parent session {}",
                    request.caller.session_id
                ))
            })?;
        let child = self
            .sessions
            .create_subagent_session(
                CreateSessionParams {
                    session_id: None,
                    context: TalosContext {
                        source_type: TalosSourceType::System,
                        provider: TalosProvider::System,
                        user_id: parent.user_id.clone(),
                        message_id: format!("msg_{}", uuid::Uuid::new_v4().simple()),
                        trace_id: parent.context.trace_id.clone(),
                        channel_id: None,
                        thread_id: None,
                        project_root: None,
                        environment: parent.context.environment.clone(),
                    },
                    target_agent: agent_id.clone(),
                    model_selection: parent.conf.model_selection.clone(),
                    conf: None,
                    workspace_id: parent.context.workspace_id.clone(),
                    project_id: parent.project_id.clone(),
                    current_directory: parent.current_directory.clone(),
                },
                request.caller.turn_ref(),
                caller.tree_root_session_id,
                child_path.clone(),
            )
            .await?;
        let child_id = child.context.session_id.clone();
        let child_agent_id = agent_id.clone();
        let start_result = self
            .turns
            .submit_subagent_turn(
                child,
                StartTurnRequest {
                    session_id: child_id.clone(),
                    input: vec![TalosInputItem::Text {
                        text: request.prompt,
                    }],
                    workspace_id: None,
                    context: None,
                    model_selection: None,
                    user_permission_mode: None,
                    project_id: None,
                    current_directory: None,
                    client_request_id: None,
                },
                agent_id,
            )
            .await;
        let start_result = match start_result {
            Ok(result) => result,
            Err(start_error) => {
                let cleanup = self.sessions.delete_subagent_session(&child_id).await;
                return match cleanup {
                    Ok(result) if result.success => Err(start_error),
                    Ok(_) => Err(TalosError::InternalError(format!(
                        "subagent start failed and child session '{child_id}' was not removed: {start_error}"
                    ))),
                    Err(cleanup_error) => Err(TalosError::InternalError(format!(
                        "subagent start failed for child session '{child_id}': {start_error}; cleanup failed: {cleanup_error}"
                    ))),
                };
            }
        };
        Ok(SubagentSpawnResult {
            subagent: SubagentRef {
                path: child_path,
                session_id: child_id.clone(),
                parent: request.caller.turn_ref(),
            },
            agent_id: child_agent_id,
            turn: AgentTurnRef {
                session_id: child_id,
                turn_id: start_result.turn_id,
            },
        })
    }

    pub(crate) async fn ensure_tree_inactive(&self, tree_root_session_id: &str) -> TalosResult<()> {
        if self
            .sessions
            .has_active_subagent_turn(tree_root_session_id)
            .await?
        {
            return Err(TalosError::ActiveTurnRunning(format!(
                "session tree '{tree_root_session_id}' has an active subagent turn"
            )));
        }
        Ok(())
    }

    fn validate_request(request: &SpawnSubagentRequest) -> TalosResult<()> {
        if request.caller.session_id.trim().is_empty()
            || request.caller.turn_id.trim().is_empty()
            || request.task_name.trim().is_empty()
            || request.prompt.trim().is_empty()
            || request.prompt.len() > MAX_SUBAGENT_PROMPT_BYTES
        {
            return Err(TalosError::ValidationError(
                format!(
                    "caller session, caller turn, task_name, and a 1 to {MAX_SUBAGENT_PROMPT_BYTES} byte prompt are required"
                ),
            ));
        }
        Ok(())
    }
}
