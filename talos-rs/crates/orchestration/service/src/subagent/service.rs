use std::sync::Arc;

use crate::session::{CreateSessionParams, SessionService};
use crate::turn::TurnService;
use crate::TimelineEventService;
use talos_error::{TalosError, TalosResult};
use talos_types::talos::SessionSource;
use talos_types::{
    AgentTurnRef, AgentUserContent, AgentUserInput, SpawnSubagentRequest, StartTurnRequest,
    SubagentRef, SubagentSpawnResult,
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
}

impl SubagentService {
    pub(crate) fn new(
        sessions: Arc<SessionService>,
        turns: Arc<TurnService>,
        timeline: Arc<TimelineEventService>,
    ) -> Self {
        Self {
            sessions,
            turns,
            timeline,
        }
    }

    pub(crate) async fn spawn(
        &self,
        request: SpawnSubagentRequest,
    ) -> TalosResult<SubagentSpawnResult> {
        Self::validate_request(&request)?;
        let caller = self.authorize_caller(&request.caller).await?;
        let agent_id = caller.agent_id.clone();
        let child_path = caller.path.join(&request.task_name)?;
        let parent = self
            .sessions
            .load_record(&request.caller.session_id)
            .await?
            .ok_or_else(|| {
                TalosError::new(
                    talos_error::ErrorCode::ResourceNotFound,
                    format!("parent session {}", request.caller.session_id),
                )
            })?;
        self.turns
            .validate_subagent_agent(&agent_id, &parent.user_id, &parent.execution_node_id)
            .await?;
        let mut child_source = SessionSource::system();
        child_source.environment = parent.environment.clone();
        let child = self
            .sessions
            .create_subagent_session(
                CreateSessionParams {
                    session_id: None,
                    source: child_source,
                    user_id: parent.user_id.clone(),
                    target_agent: agent_id.clone(),
                    execution_node_id: parent.execution_node_id.clone(),
                    model_selection: parent.conf.model_selection.clone(),
                    conf: None,
                    workspace_id: parent.workspace_id.clone(),
                    project_id: parent.project_id.clone(),
                    current_directory: parent.current_directory.clone(),
                },
                request.caller.turn_ref(),
                caller.tree_root_session_id,
                child_path.clone(),
            )
            .await?;
        let child_id = child.id.clone();
        let child_agent_id = agent_id.clone();
        let cleanup_session = child.clone();
        let start_result = self
            .turns
            .submit_subagent_turn(
                child,
                StartTurnRequest {
                    node_id: None,
                    session_id: child_id.clone(),
                    input: AgentUserInput {
                        content: vec![AgentUserContent::Text {
                            text: request.prompt,
                        }],
                    },
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
                let cleanup = self.sessions.delete_subagent_session(cleanup_session).await;
                return match cleanup {
                    Ok(true) => Err(start_error),
                    Ok(_) => Err(TalosError::internal("Subagent start failed and child session was not removed; technical details were withheld.").context(format!(
                        "subagent start failed and child session '{child_id}' was not removed: {start_error}"
                    ))),
                    Err(cleanup_error) => Err(TalosError::internal("Subagent start failed for child session; cleanup failed; technical details were withheld.").context(format!(
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
            return Err(TalosError::new(
                talos_error::ErrorCode::ActiveTurnRunning,
                format!("session tree '{tree_root_session_id}' has an active subagent turn"),
            ));
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
            return Err(TalosError::new(talos_error::ErrorCode::InvalidRequest,
                format!(
                    "caller session, caller turn, task_name, and a 1 to {MAX_SUBAGENT_PROMPT_BYTES} byte prompt are required"
                ),
            ));
        }
        Ok(())
    }
}
