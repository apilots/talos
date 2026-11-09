//! Session automation policy. The original Turn owner performs all execution.
use crate::SessionMessageQueueService;
use futures_util::future::BoxFuture;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};
use talos_agent_api::{
    ActiveTurnState, AgentEvent, AgentEventPayload, AgentMessageBlockContent, AgentMessageRole,
    AgentRunMetrics, AgentTurnRef,
};
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_store::{
    automation::goal_loop::DatabaseAutomationStore,
    session::{AgentExecutionStore, SessionRecord},
};
use talos_types::*;
mod actions;
mod explicit_dispatch;
mod settlement;
pub(crate) use explicit_dispatch::automation_object;

/// Host composition resolves protected System Skills through the existing
/// execution-node environment API. Implementations must not retain the host strongly.
pub trait AutomationSkillResolver: Send + Sync {
    fn resolve<'a>(
        &'a self,
        session: &'a SessionRecord,
        mode: AutomationMode,
    ) -> BoxFuture<'a, TalosResult<SkillSelector>>;
}
pub(crate) enum AutomationCaller<'a> {
    User,
    Model {
        caller: &'a AgentCaller,
        call_id: &'a str,
    },
}
#[derive(Clone)]
pub struct SessionAutomationService {
    pub(crate) store: DatabaseAutomationStore,
    execution: Arc<dyn AgentExecutionStore>,
    queue: SessionMessageQueueService,
    resolver: Arc<dyn AutomationSkillResolver>,
    nodes: Option<crate::AgentNodeService>,
    changed: tokio::sync::broadcast::Sender<AutomationChangedNotification>,
    gates: Arc<Mutex<HashMap<String, Weak<tokio::sync::Mutex<()>>>>>,
    declarations: Arc<Mutex<HashMap<(String, String), Declaration>>>,
    explicit_dispatch: Arc<Mutex<HashMap<String, Vec<explicit_dispatch::ExplicitDispatch>>>>,
}
#[derive(Clone)]
enum Declaration {
    Goal {
        goal_id: String,
        revision: u64,
        evidence: String,
    },
    Loop {
        loop_id: String,
        revision: u64,
        decision: LoopWakeDecision,
    },
}
impl SessionAutomationService {
    pub(crate) fn new(
        store: DatabaseAutomationStore,
        execution: Arc<dyn AgentExecutionStore>,
        queue: SessionMessageQueueService,
        resolver: Arc<dyn AutomationSkillResolver>,
    ) -> Self {
        Self {
            store,
            execution,
            queue,
            resolver,
            nodes: None,
            changed: tokio::sync::broadcast::channel(128).0,
            gates: Default::default(),
            declarations: Default::default(),
            explicit_dispatch: Default::default(),
        }
    }
    pub(crate) fn with_managed_workers(mut self, nodes: Option<crate::AgentNodeService>) -> Self {
        self.nodes = nodes;
        self
    }
    pub fn subscribe_changed(
        &self,
    ) -> tokio::sync::broadcast::Receiver<AutomationChangedNotification> {
        self.changed.subscribe()
    }
    pub(crate) fn emit(&self, session: &str) {
        let _ = self.changed.send(AutomationChangedNotification {
            session_id: session.into(),
        });
    }
    pub(crate) async fn lock(&self, session: &str) -> tokio::sync::OwnedMutexGuard<()> {
        let gate = {
            let mut gates = self.gates.lock().expect("automation gates");
            // Match the existing admission gates: live guards/waiters retain
            // their gate, while idle Session keys are reclaimed on use.
            gates.retain(|_, gate| gate.strong_count() > 0);
            if let Some(gate) = gates.get(session).and_then(Weak::upgrade) {
                gate
            } else {
                let gate = Arc::new(tokio::sync::Mutex::new(()));
                gates.insert(session.into(), Arc::downgrade(&gate));
                gate
            }
        };
        gate.lock_owned().await
    }
    pub async fn snapshot(&self, session_id: &str) -> TalosResult<SessionAutomationSnapshot> {
        self.store.snapshot(session_id).await
    }
    pub(crate) async fn initial_input(&self, turn: &AgentTurnRef) -> TalosResult<AgentUserInput> {
        let active = self
            .execution
            .get_agent_active_turn(&turn.session_id)
            .await?
            .filter(|a| a.identity == *turn)
            .ok_or_else(conflict)?;
        let initial_id = active.initial_message_id.ok_or_else(conflict)?;
        let events = self.execution.list_agent_turn_events(turn, 10_000).await?;
        for event in events {
            if let AgentEventPayload::MessageCompleted(message) = event.payload {
                if message.role == AgentMessageRole::User && message.message_id == initial_id {
                    return Ok(AgentUserInput {
                        content: message
                            .blocks
                            .into_iter()
                            .filter_map(|b| match b.content {
                                AgentMessageBlockContent::UserContent(content) => Some(content),
                                _ => None,
                            })
                            .collect(),
                    });
                }
            }
        }
        Err(TalosError::new(
            ErrorCode::ResourceNotFound,
            "The original User input is not available.",
        ))
    }
    pub(crate) async fn validate_mode(
        &self,
        session: &str,
        mode: AutomationMode,
        own_turn: Option<&str>,
        own_message: Option<&str>,
    ) -> TalosResult<()> {
        let snapshot = self.store.snapshot(session).await?;
        if snapshot
            .goal
            .is_some_and(|g| matches!(g.status, GoalStatus::Active | GoalStatus::Paused))
            || (mode == AutomationMode::Goal && !snapshot.loops.is_empty())
        {
            return Err(conflict());
        }
        for message in self.queue.list(session)?.messages {
            if Some(message.id.as_str()) == own_message {
                continue;
            }
            if let Some(pending) = explicit_automation_mode(&message.input)? {
                // Preserve the original queue identity as the canonical initial
                // User message ID. Equality of prompt text is not an identity.
                if let Some(turn_id) = own_turn {
                    if self.queue.is_reserved(&message.id, session)?
                        && self
                            .execution
                            .get_agent_active_turn(session)
                            .await?
                            .is_some_and(|active| {
                                active.identity.turn_id == turn_id
                                    && active.initial_message_id.as_deref()
                                        == Some(message.id.as_str())
                            })
                    {
                        continue;
                    }
                }
                if mode == AutomationMode::Goal || pending == AutomationMode::Goal {
                    return Err(conflict());
                }
            }
        }
        if let Some(active) = self.execution.get_agent_active_turn(session).await? {
            if Some(active.identity.turn_id.as_str()) != own_turn {
                if let Some(pending) =
                    explicit_automation_mode(&self.initial_input(&active.identity).await?)?
                {
                    if mode == AutomationMode::Goal || pending == AutomationMode::Goal {
                        return Err(conflict());
                    }
                }
                if let Some(origin) = self.store.origin(&active.identity).await? {
                    if matches!(
                        (mode, origin),
                        (AutomationMode::Goal, AutomationTurnOrigin::Loop { .. })
                            | (AutomationMode::Loop, AutomationTurnOrigin::Goal { .. })
                    ) {
                        return Err(conflict());
                    }
                }
            }
        }
        Ok(())
    }
    pub(crate) async fn validate_user_input(
        &self,
        session: &str,
        input: &AgentUserInput,
        own_message: Option<&str>,
    ) -> TalosResult<()> {
        if let Some(mode) = explicit_automation_mode(input)? {
            self.validate_mode(session, mode, None, own_message).await?;
        }
        Ok(())
    }
    pub(crate) async fn skill_input(
        &self,
        session: &SessionRecord,
        mode: AutomationMode,
        text: String,
    ) -> TalosResult<AgentUserInput> {
        let selector = self.resolver.resolve(session, mode).await?;
        if selector.name.as_deref() != Some(mode.skill_name()) {
            return Err(TalosError::new(
                ErrorCode::ServiceUnavailable,
                "The protected automation Skill is unavailable.",
            ));
        }
        let path = selector
            .path
            .filter(|p| !p.as_os_str().is_empty())
            .ok_or_else(|| {
                TalosError::new(
                    ErrorCode::ServiceUnavailable,
                    "The protected automation Skill is unavailable.",
                )
            })?;
        Ok(AgentUserInput {
            content: vec![
                AgentUserContent::Text { text },
                AgentUserContent::SkillSelection {
                    name: mode.skill_name().into(),
                    path: path.to_string_lossy().into_owned(),
                },
            ],
        })
    }
    pub(crate) async fn normalize_activation(
        &self,
        session: &SessionRecord,
        mut input: AgentUserInput,
    ) -> TalosResult<AgentUserInput> {
        if let Some(mode) = explicit_automation_mode(&input)? {
            let selected = self.skill_input(session, mode, String::new()).await?;
            input.content.retain(|c|!matches!(c,AgentUserContent::SkillSelection{name,..} if name=="goal" || name=="loop"));
            input.content.extend(
                selected
                    .content
                    .into_iter()
                    .filter(|c| matches!(c, AgentUserContent::SkillSelection { .. })),
            );
        }
        Ok(input)
    }
    pub(crate) async fn enqueue_loop(
        &self,
        session: &SessionRecord,
        task: &SessionLoop,
    ) -> TalosResult<()> {
        if let Some(active) = self.execution.get_agent_active_turn(&session.id).await? {
            if matches!(self.store.origin(&active.identity).await?,Some(AutomationTurnOrigin::Loop{loop_id,..}) if loop_id==task.id)
            {
                return Ok(());
            }
        }
        if let Some(nodes) = &self.nodes {
            if nodes
                .wake_for_managed_dispatch(&session.user_id, &session.execution_node_id)
                .await?
                != crate::agent_node::ManagedDispatch::Proceed
            {
                return Ok(());
            }
        }
        let input = self
            .skill_input(session, AutomationMode::Loop, task.prompt.clone())
            .await?;
        let mut claimed = task.clone();
        if task.next_due_at.is_some() {
            // The durable compare-and-update wins a deadline before touching
            // any process-local queue. Another embedded owner loses this CAS.
            claimed.next_due_at = None;
            match self.store.replace_loop(task, &claimed).await {
                Ok(()) => {}
                Err(error) if error.is_code(ErrorCode::Conflict) => return Ok(()),
                Err(error) => return Err(error),
            }
        }
        self.enqueue_prepared_loop(session, &claimed, input).await?;
        self.emit(&session.id);
        Ok(())
    }
    async fn enqueue_prepared_loop(
        &self,
        session: &SessionRecord,
        task: &SessionLoop,
        input: AgentUserInput,
    ) -> TalosResult<()> {
        match self.queue.add_loop(
            &session.id,
            input,
            task.id.clone(),
            format!("occ_{}", uuid::Uuid::now_v7().simple()),
        ) {
            Ok(_) => Ok(()),
            Err(error) if error.is_code(ErrorCode::MessageQueueFull) => {
                let mut next = task.clone();
                next.status = LoopStatus::Stopped;
                next.stop_reason = Some(LoopStopReason::QueueFull);
                next.reason = Some("Session queue is full.".into());
                next.next_due_at = None;
                self.store.replace_loop(task, &next).await?;
                self.emit(&session.id);
                Ok(())
            }
            Err(error) => Err(error),
        }
    }
    /// Original queue admission materializes the latest configuration. The Store
    /// checks the same revision/state again inside durable admission.
    pub(crate) async fn prepare_loop(
        &self,
        session: &SessionRecord,
        loop_id: &str,
        occurrence_id: &str,
    ) -> TalosResult<Option<(AgentUserInput, AutomationTurnOrigin)>> {
        let task = match self.store.loop_task(loop_id).await {
            Ok(task) => task,
            Err(e) if e.is_code(ErrorCode::ResourceNotFound) => return Ok(None),
            Err(e) => return Err(e),
        };
        if task.session_id != session.id || task.status != LoopStatus::Active {
            return Ok(None);
        }
        Ok(Some((
            self.skill_input(session, AutomationMode::Loop, task.prompt.clone())
                .await?,
            AutomationTurnOrigin::Loop {
                loop_id: task.id,
                occurrence_id: occurrence_id.into(),
                config_revision: task.config_revision,
            },
        )))
    }
}
fn conflict() -> TalosError {
    TalosError::new(
        ErrorCode::Conflict,
        "Session automation conflicts with an existing definition or activation.",
    )
}
fn invalid(message: &'static str) -> TalosError {
    TalosError::new(ErrorCode::InvalidRequest, message)
}
fn changed_revision() -> TalosError {
    TalosError::new(
        ErrorCode::Conflict,
        "Automation configuration changed; refresh before editing.",
    )
}
fn next_revision(revision: u64) -> TalosResult<u64> {
    revision
        .checked_add(1)
        .ok_or_else(|| TalosError::internal("Automation revision overflow."))
}
fn next_due(now: i64, minutes: u32) -> TalosResult<i64> {
    now.checked_add(i64::from(minutes) * 60)
        .ok_or_else(|| invalid("Automation interval is too large."))
}

fn turn_key(turn: &AgentTurnRef) -> (String, String) {
    (turn.session_id.clone(), turn.turn_id.clone())
}

impl SessionAutomationService {
    pub(crate) async fn failed_loop_admission(
        &self,
        loop_id: &str,
        error: &TalosError,
    ) -> TalosResult<()> {
        let task = match self.store.loop_task(loop_id).await {
            Ok(task) => task,
            Err(e) if e.is_code(ErrorCode::ResourceNotFound) => return Ok(()),
            Err(e) => return Err(e),
        };
        if task.status != LoopStatus::Active {
            self.queue.remove_loop(&task.session_id, &task.id)?;
            return Ok(());
        }
        if !error.is_code(ErrorCode::Conflict) && !error.is_code(ErrorCode::ActiveTurnRunning) {
            self.stop(
                &task,
                LoopStopReason::ExecutionFailed,
                Some(error.to_api_error().message),
                chrono::Utc::now().timestamp(),
            )
            .await?;
        }
        Ok(())
    }
}
