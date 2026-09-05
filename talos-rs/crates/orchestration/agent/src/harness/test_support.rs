use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use talos_agent_api::{
    AgentContinuationRef, AgentMessageBlockContent, AgentMessageRole, AgentPendingInput,
    AgentStopReason, AgentTurnRef,
};
use talos_config::{EffectiveWorkspace, ProviderModelConfig};
use talos_types::{ModelInfo, ModelRef, ModelSelection, TokenUsage, UserPermissionMode};
use tokio::sync::Notify;

use super::{AgentError, AgentResult};
use crate::{
    AgentCapabilities, AgentContext, AgentOperationControl, AgentProvider, AgentRunParams,
    AgentSession, AgentSessionContext, SessionCompactionParams, SessionCompactionResult,
    SessionTurnParams, SessionTurnResult,
};

#[derive(Debug, Clone, Copy)]
pub(super) enum FakeMode {
    Release,
    Input,
    PendingInput,
    RejectInput,
    Waiting,
    OpenThenFail,
    FailAcquisition,
    UnconfirmedAcquisition,
}

#[derive(Clone)]
pub(super) struct FakeProvider {
    mode: FakeMode,
    capabilities: AgentCapabilities,
    pub(super) started: Arc<AtomicBool>,
    started_notify: Arc<Notify>,
    pub(super) release: Arc<Notify>,
    pub(super) input_received: Arc<AtomicBool>,
    pub(super) clear_called: Arc<AtomicBool>,
    clear_fails: bool,
    block_session_acquisition: bool,
    session_requested: Arc<AtomicBool>,
    session_requested_notify: Arc<Notify>,
    session_release: Arc<Notify>,
    pub(super) close_called: Arc<AtomicBool>,
    close_fails: bool,
    pub(super) shutdown_called: Arc<AtomicBool>,
}

#[derive(Clone)]
pub(super) struct FakeSession {
    provider: FakeProvider,
}

impl FakeProvider {
    pub(super) fn new(mode: FakeMode) -> Self {
        Self {
            mode,
            capabilities: AgentCapabilities {
                rich_input: Vec::new(),
                dynamic_input: true,
                user_actions: Vec::new(),
                durable_continuation: true,
                context_compaction: true,
            },
            started: Arc::new(AtomicBool::new(false)),
            started_notify: Arc::new(Notify::new()),
            release: Arc::new(Notify::new()),
            input_received: Arc::new(AtomicBool::new(false)),
            clear_called: Arc::new(AtomicBool::new(false)),
            clear_fails: false,
            block_session_acquisition: false,
            session_requested: Arc::new(AtomicBool::new(false)),
            session_requested_notify: Arc::new(Notify::new()),
            session_release: Arc::new(Notify::new()),
            close_called: Arc::new(AtomicBool::new(false)),
            close_fails: false,
            shutdown_called: Arc::new(AtomicBool::new(false)),
        }
    }

    pub(super) fn with_clear_failure(mut self) -> Self {
        self.clear_fails = true;
        self
    }

    pub(super) fn with_blocked_session_acquisition(mut self) -> Self {
        self.block_session_acquisition = true;
        self
    }

    pub(super) fn with_close_failure(mut self) -> Self {
        self.close_fails = true;
        self
    }

    pub(super) fn with_capabilities(mut self, capabilities: AgentCapabilities) -> Self {
        self.capabilities = capabilities;
        self
    }

    pub(super) async fn wait_started(&self) {
        while !self.started.load(Ordering::Acquire) {
            self.started_notify.notified().await;
        }
    }

    pub(super) async fn wait_session_requested(&self) {
        while !self.session_requested.load(Ordering::Acquire) {
            self.session_requested_notify.notified().await;
        }
    }
}

impl AgentProvider for FakeProvider {
    type Session = FakeSession;

    fn capabilities(&self) -> &AgentCapabilities {
        &self.capabilities
    }

    async fn session(
        &self,
        _context: AgentSessionContext,
        control: AgentOperationControl,
    ) -> AgentResult<Self::Session> {
        self.session_requested.store(true, Ordering::Release);
        self.session_requested_notify.notify_waiters();
        if self.block_session_acquisition {
            self.session_release.notified().await;
        }
        if matches!(
            self.mode,
            FakeMode::FailAcquisition | FakeMode::UnconfirmedAcquisition
        ) {
            let mode = self.mode;
            control
                .tasks()
                .spawn(move |_| async move {
                    if matches!(mode, FakeMode::UnconfirmedAcquisition) {
                        talos_run_control::RunTaskCompletion::CleanupUnconfirmed(())
                    } else {
                        talos_run_control::RunTaskCompletion::Confirmed(())
                    }
                })
                .expect("register Session resource")
                .await
                .expect("finish Session resource");
            return Err(AgentError::provider_error(
                "fake Session acquisition failed",
            ));
        }
        Ok(FakeSession {
            provider: self.clone(),
        })
    }

    async fn shutdown(&self, _control: AgentOperationControl) -> AgentResult<()> {
        self.shutdown_called.store(true, Ordering::Release);
        Ok(())
    }
}

impl AgentSession for FakeSession {
    async fn turn(&self, mut turn: SessionTurnParams) -> AgentResult<SessionTurnResult> {
        self.provider.started.store(true, Ordering::Release);
        self.provider.started_notify.notify_waiters();
        match self.provider.mode {
            FakeMode::Release => {
                tokio::select! {
                    () = self.provider.release.notified() => completed(),
                    () = turn.control.cancellation().requested() => interrupted(),
                }
            }
            FakeMode::Input => {
                tokio::select! {
                    delivery = turn.input.receive() => {
                        let delivery = delivery.expect("Harness input channel remains open");
                        self.provider.input_received.store(true, Ordering::Release);
                        delivery.accept();
                        completed()
                    }
                    () = turn.control.cancellation().requested() => interrupted(),
                }
            }
            FakeMode::PendingInput => {
                let _delivery = turn
                    .input
                    .receive()
                    .await
                    .expect("Harness input channel remains open");
                self.provider.input_received.store(true, Ordering::Release);
                turn.control.cancellation().requested().await;
                interrupted()
            }
            FakeMode::RejectInput => {
                let delivery = turn
                    .input
                    .receive()
                    .await
                    .expect("Harness input channel remains open");
                delivery.reject(AgentError::input_rejected("fake Provider rejected input"));
                turn.control.cancellation().requested().await;
                interrupted()
            }
            FakeMode::Waiting => {
                turn.events
                    .update_metrics(talos_agent_api::AgentRunMetrics {
                        usage: TokenUsage {
                            input_tokens: 2,
                            output_tokens: 1,
                            total_tokens: 3,
                            ..TokenUsage::default()
                        },
                        ..talos_agent_api::AgentRunMetrics::default()
                    })
                    .await?;
                Ok(SessionTurnResult::WaitingForInput {
                    pending: Box::new(AgentPendingInput::Message {
                        action_id: "action-1".to_string(),
                        prompt: "More input".to_string(),
                    }),
                    continuation: AgentContinuationRef {
                        opaque_id: "continuation-1".to_string(),
                    },
                })
            }
            FakeMode::OpenThenFail => {
                let message = turn.events.start_message(AgentMessageRole::Agent).await?;
                let _block = turn
                    .events
                    .start_block(
                        &message,
                        AgentMessageBlockContent::AssistantText(
                            talos_agent_api::AgentAssistantText {
                                phase: talos_agent_api::AgentAssistantTextPhase::Commentary,
                                text: "partial".to_string(),
                            },
                        ),
                    )
                    .await?;
                Err(AgentError::provider_error(
                    "fake Provider failed with open output",
                ))
            }
            FakeMode::FailAcquisition | FakeMode::UnconfirmedAcquisition => {
                unreachable!("failed acquisition cannot execute a Turn")
            }
        }
    }

    async fn compact(
        &self,
        _params: SessionCompactionParams,
    ) -> AgentResult<SessionCompactionResult> {
        Ok(SessionCompactionResult {
            usage: TokenUsage {
                input_tokens: 1,
                output_tokens: 2,
                total_tokens: 3,
                ..TokenUsage::default()
            },
        })
    }

    async fn clear(&self, _control: AgentOperationControl) -> AgentResult<()> {
        self.provider.clear_called.store(true, Ordering::Release);
        if self.provider.clear_fails {
            return Err(AgentError::provider_error("fake clear failed"));
        }
        Ok(())
    }

    async fn close(&self, _control: AgentOperationControl) -> AgentResult<()> {
        self.provider.close_called.store(true, Ordering::Release);
        if self.provider.close_fails {
            return Err(AgentError::provider_error("fake close failed"));
        }
        Ok(())
    }
}

pub(super) fn run_params(identity: AgentTurnRef, message_id: &str) -> AgentRunParams {
    AgentRunParams {
        context: context(identity.clone()),
        input: user_message(&identity, message_id),
        checkpoint: None,
        continuation: None,
    }
}

pub(super) const TEST_AGENT_ID: &str = "agent-1";

pub(super) fn user_message(
    identity: &AgentTurnRef,
    message_id: &str,
) -> talos_agent_api::AgentMessage {
    talos_agent_api::AgentMessage {
        message_id: message_id.to_string(),
        turn_id: identity.turn_id.clone(),
        role: talos_agent_api::AgentMessageRole::User,
        status: talos_agent_api::AgentMessageStatus::Completed,
        blocks: Vec::new(),
        created_at: 1,
        completed_at: Some(1),
    }
}

pub(super) fn context(identity: AgentTurnRef) -> AgentContext {
    AgentContext {
        session: AgentSessionContext {
            agent_id: "agent-1".to_string(),
            session_id: identity.session_id,
            workspace: workspace(),
        },
        turn_id: identity.turn_id,
        project_id: None,
        working_directory: None,
        environment: None,
        model: ModelInfo::from_provider_model(
            "test".to_string(),
            ProviderModelConfig {
                id: "model".to_string(),
                name: "Model".to_string(),
                context_window: 32_768,
                reasoning: false,
                reasoning_effort: Default::default(),
                speed: Default::default(),
                input: Vec::new(),
                cost: Default::default(),
            },
        ),
        model_selection: ModelSelection::without_reasoning(ModelRef {
            provider: "test".to_string(),
            id: "model".to_string(),
        }),
        permission_mode: UserPermissionMode::FullAccess,
    }
}

pub(super) fn session_params() -> AgentSessionContext {
    AgentSessionContext {
        agent_id: "agent-1".to_string(),
        session_id: "session-1".to_string(),
        workspace: workspace(),
    }
}

fn workspace() -> EffectiveWorkspace {
    EffectiveWorkspace {
        workspace_id: "workspace-1".to_string(),
        user_id: "user-1".to_string(),
        root: "/tmp/workspace-1".into(),
    }
}

fn completed() -> AgentResult<SessionTurnResult> {
    Ok(SessionTurnResult::Completed {
        stop_reason: AgentStopReason::EndTurn,
    })
}

fn interrupted() -> AgentResult<SessionTurnResult> {
    Err(AgentError::new(
        crate::AgentErrorKind::Interrupted,
        "fake Provider interrupted",
    ))
}
