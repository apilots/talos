use std::time::Duration;

use talos_error::{TalosError, TalosResult};
use talos_store::session::{ActiveTurnStatus, SessionTreeIdentity};
use talos_types::talos::TalosInputItem;
use talos_types::{
    AgentCaller, AgentPath, AgentTarget, AgentTreeNode, AgentTreeSnapshot, AgentTurnRef,
    FollowUpSubagentRequest, InterruptSubagentRequest, ListSubagentsRequest, SendMessageResult,
    SendSubagentMessageRequest, StartTurnRequest, SubagentOutcome, SubagentRef, SubagentSnapshot,
    WaitSubagentRequest,
};
use tokio::sync::broadcast;
use tokio::time::Instant;

use super::{service::SubagentService, MAX_AGENT_TREE_NODES};

const MAX_WAIT_DURATION: Duration = Duration::from_secs(60);
const MAX_FOLLOW_UP_PROMPT_BYTES: usize = 32 * 1024;
const MAX_MESSAGE_BYTES: usize = 32 * 1024;
const MAX_AGENT_TREE_RESPONSE_BYTES: usize = 32 * 1024;

/// Authorized location of the caller inside one Session Agent tree.
pub(super) struct AuthorizedCaller {
    pub(super) tree_root_session_id: String,
    pub(super) path: AgentPath,
    pub(super) agent_id: String,
}

impl SubagentService {
    pub(crate) async fn send_message(
        &self,
        request: SendSubagentMessageRequest,
    ) -> TalosResult<SendMessageResult> {
        if request.message.trim().is_empty() || request.message.len() > MAX_MESSAGE_BYTES {
            return Err(TalosError::public(
                talos_error::ErrorCode::InvalidRequest,
                format!("message must contain 1 to {MAX_MESSAGE_BYTES} bytes"),
            ));
        }
        let caller = self.authorize_caller(&request.caller).await?;
        let (child, subagent, agent_id) = self.resolve_target(&caller, &request.target).await?;
        let active = self
            .timeline
            .get_active_turn(&subagent.session_id)
            .await?
            .ok_or_else(|| {
                TalosError::public(
                    talos_error::ErrorCode::Conflict,
                    format!("subagent '{}' has no active turn", subagent.session_id),
                )
            })?;
        if active.status != ActiveTurnStatus::Running {
            return Err(TalosError::public(
                talos_error::ErrorCode::Conflict,
                format!("subagent '{}' is not running", subagent.session_id),
            ));
        }
        self.turns
            .submit_subagent_message(
                child,
                active.turn_id.clone(),
                agent_id.clone(),
                request.message,
            )
            .await?;
        Ok(SendMessageResult {
            subagent: subagent.clone(),
            agent_id,
            turn: AgentTurnRef {
                session_id: subagent.session_id,
                turn_id: active.turn_id,
            },
            accepted: true,
        })
    }

    pub(crate) async fn wait(&self, request: WaitSubagentRequest) -> TalosResult<SubagentSnapshot> {
        Self::validate_wait_request(&request)?;
        let caller = self.authorize_caller(&request.caller).await?;
        let (_, subagent, _) = self.resolve_target(&caller, &request.target).await?;

        let mut live_events = self.timeline.subscribe_live_events();
        let mut snapshot = self.snapshot(subagent.clone()).await?;
        let turn_id = Self::outcome_turn_id(&snapshot.outcome).to_string();
        if Self::is_terminal(&snapshot.outcome) || request.timeout_ms == 0 {
            return Ok(snapshot);
        }

        let deadline =
            Instant::now() + Duration::from_millis(request.timeout_ms).min(MAX_WAIT_DURATION);
        loop {
            let received = tokio::time::timeout_at(deadline, live_events.recv()).await;
            match received {
                Err(_) => return self.snapshot_exact(subagent, &turn_id).await,
                Ok(Ok(event))
                    if event.session_id == subagent.session_id && event.turn_id == turn_id =>
                {
                    snapshot = self.snapshot_exact(subagent.clone(), &turn_id).await?;
                    if Self::is_terminal(&snapshot.outcome) {
                        return Ok(snapshot);
                    }
                }
                Ok(Ok(_)) => {}
                Ok(Err(broadcast::error::RecvError::Lagged(_))) => {
                    snapshot = self.snapshot_exact(subagent, &turn_id).await?;
                    if Self::is_terminal(&snapshot.outcome) {
                        return Ok(snapshot);
                    }
                    return Err(TalosError::public(
                        talos_error::ErrorCode::Conflict,
                        "subagent event stream lagged; retry wait from durable state".to_string(),
                    ));
                }
                Ok(Err(broadcast::error::RecvError::Closed)) => {
                    return Err(TalosError::internal(
                        "subagent event stream closed".to_string(),
                    ));
                }
            }
        }
    }

    pub(crate) async fn interrupt(
        &self,
        request: InterruptSubagentRequest,
    ) -> TalosResult<SubagentSnapshot> {
        let caller = self.authorize_caller(&request.caller).await?;
        let (child, subagent, agent_id) = self.resolve_target(&caller, &request.target).await?;
        let active = self
            .timeline
            .get_active_turn(&subagent.session_id)
            .await?
            .ok_or_else(|| {
                TalosError::public(
                    talos_error::ErrorCode::Conflict,
                    format!("subagent '{}' has no turn state", subagent.session_id),
                )
            })?;
        match active.status {
            ActiveTurnStatus::Completed | ActiveTurnStatus::Failed => self.snapshot(subagent).await,
            ActiveTurnStatus::Cancelling => self.snapshot(subagent).await,
            ActiveTurnStatus::Running | ActiveTurnStatus::WaitingUserAction => {
                self.turns
                    .interrupt_subagent_turn(child, active.turn_id.clone(), agent_id)
                    .await?;
                self.snapshot(subagent).await
            }
        }
    }

    pub(crate) async fn follow_up(
        &self,
        request: FollowUpSubagentRequest,
    ) -> TalosResult<SubagentSnapshot> {
        if request.prompt.trim().is_empty() || request.prompt.len() > MAX_FOLLOW_UP_PROMPT_BYTES {
            return Err(TalosError::public(
                talos_error::ErrorCode::InvalidRequest,
                format!("follow-up prompt must contain 1 to {MAX_FOLLOW_UP_PROMPT_BYTES} bytes"),
            ));
        }
        let caller = self.authorize_caller(&request.caller).await?;
        let (child, subagent, agent_id) = self.resolve_target(&caller, &request.target).await?;
        if let Some(active) = self.timeline.get_active_turn(&subagent.session_id).await? {
            if matches!(
                active.status,
                ActiveTurnStatus::Running
                    | ActiveTurnStatus::WaitingUserAction
                    | ActiveTurnStatus::Cancelling
            ) {
                return Err(TalosError::public(
                    talos_error::ErrorCode::ActiveTurnRunning,
                    format!(
                        "subagent '{}' already has an active turn",
                        subagent.session_id
                    ),
                ));
            }
        }
        let result = self
            .turns
            .submit_subagent_turn(
                child,
                StartTurnRequest {
                    session_id: subagent.session_id.clone(),
                    input: vec![TalosInputItem::Text {
                        text: request.prompt,
                    }],
                    context: None,
                    model_selection: None,
                    user_permission_mode: None,
                    project_id: None,
                    current_directory: None,
                    client_request_id: None,
                },
                agent_id,
            )
            .await?;
        self.snapshot_exact(subagent, &result.turn_id).await
    }

    pub(crate) async fn list(
        &self,
        request: ListSubagentsRequest,
    ) -> TalosResult<AgentTreeSnapshot> {
        let caller = self.authorize_caller(&request.caller).await?;
        self.list_tree(caller.tree_root_session_id, caller.path)
            .await
    }

    async fn list_tree(
        &self,
        tree_root_session_id: String,
        visible_root: AgentPath,
    ) -> TalosResult<AgentTreeSnapshot> {
        let records = self
            .sessions
            .list_subagent_tree_records(tree_root_session_id, Some(MAX_AGENT_TREE_NODES + 1))
            .await?;
        let mut data = Vec::with_capacity(records.len());
        for record in records {
            let SessionTreeIdentity::Subagent { parent, path, .. } = record.tree_identity else {
                return Err(TalosError::internal(format!(
                    "subagent tree contains Primary Session '{}'",
                    record.id
                )));
            };
            if !visible_root.is_ancestor_of(&path) {
                continue;
            }
            data.push(
                self.snapshot_with_agent_id(
                    SubagentRef {
                        path,
                        session_id: record.id,
                        parent,
                    },
                    record.agent_id,
                )
                .await?,
            );
        }
        if data.len() > MAX_AGENT_TREE_NODES {
            return Err(TalosError::public(
                talos_error::ErrorCode::InvalidRequest,
                format!("agent tree exceeds the {MAX_AGENT_TREE_NODES} node read limit"),
            ));
        }
        let tree = Self::build_agent_tree(data, &visible_root)?;
        let encoded = serde_json::to_vec(&tree).map_err(|error| {
            TalosError::internal(format!("failed to serialize agent tree snapshot: {error}"))
        })?;
        if encoded.len() > MAX_AGENT_TREE_RESPONSE_BYTES {
            return Err(TalosError::public(talos_error::ErrorCode::InvalidRequest, format!(
                "agent tree snapshot exceeds the {MAX_AGENT_TREE_RESPONSE_BYTES} byte response limit"
            )));
        }
        Ok(tree)
    }

    pub(super) async fn authorize_caller(
        &self,
        caller: &AgentCaller,
    ) -> TalosResult<AuthorizedCaller> {
        if caller.session_id.trim().is_empty()
            || caller.turn_id.trim().is_empty()
            || caller.agent_id.trim().is_empty()
        {
            return Err(TalosError::public(
                talos_error::ErrorCode::InvalidRequest,
                "a trusted caller session, turn, and agent are required".to_string(),
            ));
        }
        let record = self
            .sessions
            .get_session_record(&caller.session_id)
            .await?
            .ok_or_else(|| {
                TalosError::public(
                    talos_error::ErrorCode::ResourceNotFound,
                    format!("caller session {}", caller.session_id),
                )
            })?;
        let active = self
            .timeline
            .get_active_turn(&caller.session_id)
            .await?
            .ok_or_else(|| {
                TalosError::public(
                    talos_error::ErrorCode::InvalidRequest,
                    format!("caller session '{}' has no active turn", caller.session_id),
                )
            })?;
        if active.turn_id != caller.turn_id || active.status != ActiveTurnStatus::Running {
            return Err(TalosError::public(
                talos_error::ErrorCode::InvalidRequest,
                format!(
                    "turn '{}' is not the active running turn for caller session '{}'",
                    caller.turn_id, caller.session_id
                ),
            ));
        }
        if record.agent_id != caller.agent_id {
            return Err(TalosError::public(
                talos_error::ErrorCode::InvalidRequest,
                format!(
                    "caller agent '{}' does not own session '{}'",
                    caller.agent_id, caller.session_id
                ),
            ));
        }
        match record.tree_identity {
            SessionTreeIdentity::Primary => Ok(AuthorizedCaller {
                tree_root_session_id: caller.session_id.clone(),
                path: AgentPath::root(),
                agent_id: record.agent_id,
            }),
            SessionTreeIdentity::Subagent {
                tree_root_session_id,
                path,
                ..
            } => Ok(AuthorizedCaller {
                tree_root_session_id,
                path,
                agent_id: record.agent_id,
            }),
        }
    }

    async fn resolve_target(
        &self,
        caller: &AuthorizedCaller,
        target: &AgentTarget,
    ) -> TalosResult<(talos_types::TalosSession, SubagentRef, String)> {
        let path = target.resolve_descendant(&caller.path)?.ok_or_else(|| {
            TalosError::public(
                talos_error::ErrorCode::ResourceNotFound,
                format!("subagent {}", target.as_str()),
            )
        })?;
        let record = self
            .sessions
            .get_subagent_record(&caller.tree_root_session_id, &path)
            .await?
            .ok_or_else(|| {
                TalosError::public(
                    talos_error::ErrorCode::ResourceNotFound,
                    format!("subagent {}", target.as_str()),
                )
            })?;
        let SessionTreeIdentity::Subagent { parent, .. } = record.tree_identity else {
            return Err(TalosError::internal(format!(
                "resolved subagent '{}' has Primary identity",
                record.id
            )));
        };
        let session = self
            .sessions
            .get_session(&record.id)
            .await?
            .ok_or_else(|| {
                TalosError::public(
                    talos_error::ErrorCode::ResourceNotFound,
                    format!("subagent {}", target.as_str()),
                )
            })?;
        let agent_id = record.agent_id.clone();
        Ok((
            session,
            SubagentRef {
                path,
                session_id: record.id,
                parent,
            },
            agent_id,
        ))
    }

    fn validate_wait_request(request: &WaitSubagentRequest) -> TalosResult<()> {
        if Duration::from_millis(request.timeout_ms) > MAX_WAIT_DURATION {
            return Err(TalosError::public(
                talos_error::ErrorCode::InvalidRequest,
                "wait timeout exceeds 60000 milliseconds".to_string(),
            ));
        }
        Ok(())
    }

    fn build_agent_tree(
        mut snapshots: Vec<SubagentSnapshot>,
        visible_root: &AgentPath,
    ) -> TalosResult<AgentTreeSnapshot> {
        snapshots.sort_by(|left, right| {
            left.subagent
                .path
                .depth()
                .cmp(&right.subagent.path.depth())
                .then_with(|| left.subagent.path.cmp(&right.subagent.path))
        });
        let mut pending_children =
            std::collections::BTreeMap::<AgentPath, Vec<AgentTreeNode>>::new();
        let mut root_children = Vec::new();
        for snapshot in snapshots.into_iter().rev() {
            let path = snapshot.subagent.path.clone();
            let mut node = AgentTreeNode {
                snapshot,
                children: pending_children.remove(&path).unwrap_or_default(),
            };
            Self::sort_agent_tree_nodes(&mut node.children);
            match path.parent() {
                Some(parent) if parent == *visible_root => root_children.push(node),
                Some(parent) => pending_children.entry(parent).or_default().push(node),
                None => {
                    return Err(TalosError::internal(
                        "subagent tree unexpectedly contains the root path".to_string(),
                    ));
                }
            }
        }
        if let Some((missing_parent, _)) = pending_children.into_iter().next() {
            return Err(TalosError::internal(format!(
                "subagent tree is missing parent path '{missing_parent}'"
            )));
        }
        Self::sort_agent_tree_nodes(&mut root_children);
        Ok(AgentTreeSnapshot {
            children: root_children,
        })
    }

    fn sort_agent_tree_nodes(nodes: &mut [AgentTreeNode]) {
        nodes.sort_by(|left, right| {
            left.snapshot
                .subagent
                .path
                .cmp(&right.snapshot.subagent.path)
        });
        for node in nodes {
            Self::sort_agent_tree_nodes(&mut node.children);
        }
    }

    fn is_terminal(outcome: &SubagentOutcome) -> bool {
        matches!(
            outcome,
            SubagentOutcome::Completed { .. }
                | SubagentOutcome::Failed { .. }
                | SubagentOutcome::Interrupted { .. }
        )
    }

    fn outcome_turn_id(outcome: &SubagentOutcome) -> &str {
        match outcome {
            SubagentOutcome::Running { turn_id }
            | SubagentOutcome::WaitingUserAction { turn_id }
            | SubagentOutcome::Cancelling { turn_id }
            | SubagentOutcome::Completed { turn_id, .. }
            | SubagentOutcome::Failed { turn_id, .. }
            | SubagentOutcome::Interrupted { turn_id } => turn_id,
        }
    }
}
