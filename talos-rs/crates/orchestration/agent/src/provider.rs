use std::future::Future;

use talos_run_control::{ActiveRunControl, Cancellation, RunTaskGroup};
use talos_types::TokenUsage;
use tokio::sync::{mpsc, oneshot};

use talos_agent_api::{AgentContinuationRef, AgentMessage, AgentPendingInput, AgentStopReason};

use crate::{AgentCapabilities, AgentContext, AgentEventSink, AgentResult, AgentSessionContext};

const SESSION_TURN_INPUT_CAPACITY: usize = 32;

/// One admitted Turn execution delivered by Harness to an Agent Session.
///
/// This is the single Turn boundary object: immutable input and the live
/// capabilities required by the running Turn travel together. It does not
/// expose Harness settlement, product persistence, or transport objects.
pub struct SessionTurnParams {
    pub context: AgentContext,
    pub initial_input: AgentMessage,
    pub continuation: Option<AgentContinuationRef>,
    pub input: SessionTurnInputReceiver,
    pub events: AgentEventSink,
    pub control: AgentOperationControl,
}

/// One admitted context-compaction execution inside an Agent Session.
#[derive(Clone)]
pub struct SessionCompactionParams {
    pub context: AgentContext,
    pub events: AgentEventSink,
    pub control: AgentOperationControl,
}

/// Typed result of one Agent Session context compaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionCompactionResult {
    pub usage: TokenUsage,
}

impl std::fmt::Debug for SessionTurnParams {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SessionTurnParams")
            .field("context", &self.context)
            .field("initial_input", &self.initial_input)
            .field("continuation", &self.continuation)
            .field("input", &self.input)
            .field("events", &"AgentEventSink")
            .field("control", &self.control)
            .finish()
    }
}

impl std::fmt::Debug for SessionCompactionParams {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SessionCompactionParams")
            .field("context", &self.context)
            .field("events", &"AgentEventSink")
            .field("control", &self.control)
            .finish()
    }
}

/// Restricted cancellation and task-registration capabilities for one scope.
///
/// Harness creates this value from one `ActiveRunControl`. Provider and Session
/// may observe cancellation and register owned work, but cannot settle or
/// force-abort the scope or combine capabilities from different controls.
#[derive(Clone)]
pub struct AgentOperationControl {
    cancellation: Cancellation,
    tasks: RunTaskGroup,
}

impl AgentOperationControl {
    /// Borrow restricted Provider capabilities from one Harness lifecycle owner.
    pub(crate) fn from_run_control(control: &ActiveRunControl) -> Self {
        Self {
            cancellation: control.cancellation(),
            tasks: control.tasks(),
        }
    }

    /// Return the cooperative cancellation signal for this Provider call.
    pub fn cancellation(&self) -> Cancellation {
        self.cancellation.clone()
    }

    /// Return the task group that must own every spawned task or process.
    pub fn tasks(&self) -> RunTaskGroup {
        self.tasks.clone()
    }
}

impl std::fmt::Debug for AgentOperationControl {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("AgentOperationControl")
    }
}

/// Harness-owned sender for accepted input targeting one live invocation.
#[derive(Clone)]
pub(crate) struct SessionTurnInputSender {
    sender: mpsc::Sender<SessionTurnInputDelivery>,
}

impl SessionTurnInputSender {
    /// Deliver one already-admitted input message to the live Provider.
    pub(crate) async fn send(&self, input: AgentMessage) -> AgentResult<()> {
        let (accepted_tx, accepted_rx) = oneshot::channel();
        self.sender
            .send(SessionTurnInputDelivery { input, accepted_tx })
            .await
            .map_err(|_| {
                talos_error::TalosError::new(
                    talos_error::ErrorCode::InternalError,
                    "provider input receiver closed before delivery",
                )
            })?;
        accepted_rx.await.map_err(|_| {
            talos_error::TalosError::new(
                talos_error::ErrorCode::InternalError,
                "Provider dropped input without acknowledgement",
            )
        })?
    }
}

/// One live input whose final acceptance must be acknowledged by Provider.
pub struct SessionTurnInputDelivery {
    input: AgentMessage,
    accepted_tx: oneshot::Sender<AgentResult<()>>,
}

impl SessionTurnInputDelivery {
    /// Borrow the canonical User message while Provider validates it.
    pub fn input(&self) -> &AgentMessage {
        &self.input
    }

    /// Consume the delivery and confirm safe-boundary acceptance.
    pub fn accept(self) {
        let _ = self.accepted_tx.send(Ok(()));
    }

    /// Consume the delivery and report a typed rejection.
    pub fn reject(self, error: talos_error::TalosError) {
        let _ = self.accepted_tx.send(Err(error));
    }
}

/// Session-owned receiver for typed input sent during one live Turn.
pub struct SessionTurnInputReceiver {
    receiver: mpsc::Receiver<SessionTurnInputDelivery>,
}

impl SessionTurnInputReceiver {
    /// Create the bounded input channel shared by Harness and Provider.
    pub(crate) fn channel() -> (SessionTurnInputSender, Self) {
        let (sender, receiver) = mpsc::channel(SESSION_TURN_INPUT_CAPACITY);
        (SessionTurnInputSender { sender }, Self { receiver })
    }

    /// Receive the next accepted input, or `None` after Harness closes input.
    pub async fn receive(&mut self) -> Option<SessionTurnInputDelivery> {
        self.receiver.recv().await
    }
}

impl std::fmt::Debug for SessionTurnInputReceiver {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("SessionTurnInputReceiver")
    }
}

impl std::fmt::Debug for SessionTurnInputDelivery {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("SessionTurnInputDelivery")
    }
}

/// Agent Session outcome of one Turn invocation before Harness settlement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionTurnResult {
    Completed {
        stop_reason: AgentStopReason,
    },
    WaitingForInput {
        pending: Box<AgentPendingInput>,
        continuation: AgentContinuationRef,
    },
}

/// Pluggable Agent implementation contract hosted by Talos Agent Harness.
///
/// Implementations create in-process Agent Sessions and own Provider-wide
/// resources. Turn execution and Session state management belong to the
/// associated `AgentSession`; product admission, history, persistence, routing,
/// and terminal settlement remain in Talos.
pub trait AgentProvider: Clone + Send + Sync + 'static {
    /// In-process Session handle created by this Provider.
    type Session: AgentSession;

    /// Return the operation gates supported by this Provider instance.
    fn capabilities(&self) -> &AgentCapabilities;

    /// Acquire or reconstruct one in-process Agent Session handle.
    ///
    /// The supplied control owns Session-lifetime work and remains valid until
    /// Harness closes the returned handle; it is not a Turn control.
    fn session(
        &self,
        context: AgentSessionContext,
        control: AgentOperationControl,
    ) -> impl Future<Output = AgentResult<Self::Session>> + Send;

    /// Release all idle process-wide Provider resources during shutdown.
    fn shutdown(
        &self,
        control: AgentOperationControl,
    ) -> impl Future<Output = AgentResult<()>> + Send;
}

/// In-process execution and management handle for one Talos Agent Session.
///
/// Implementations may own Provider-private transient or durable state, but
/// must not import Talos Session Records, Timeline, idempotency, or recovery
/// policy. Every spawned task or process must use the supplied control.
pub trait AgentSession: Send + Sync + 'static {
    /// Execute or continue one logical Turn invocation.
    fn turn(
        &self,
        turn: SessionTurnParams,
    ) -> impl Future<Output = AgentResult<SessionTurnResult>> + Send;

    /// Compact Provider-owned context for this Session.
    fn compact(
        &self,
        params: SessionCompactionParams,
    ) -> impl Future<Output = AgentResult<SessionCompactionResult>> + Send;

    /// Delete reconstructable Provider-owned state while keeping the handle usable.
    fn clear(&self, control: AgentOperationControl)
        -> impl Future<Output = AgentResult<()>> + Send;

    /// Release this Session's live Provider resources without deleting resumable state.
    fn close(&self, control: AgentOperationControl)
        -> impl Future<Output = AgentResult<()>> + Send;
}
