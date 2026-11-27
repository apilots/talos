use std::future::Future;
use std::pin::Pin;

use tokio::sync::{mpsc, oneshot};

use talos_agent_api::{
    AgentContinuationRef, AgentEvent, AgentMessage, AgentMessageRole, AgentMessageStatus,
    AgentRunMetrics, AgentTurnRef,
};
use talos_error::TalosError;

use crate::{AgentContext, AgentResult, AgentRunCheckpoint};

type AgentPermitFuture = Pin<Box<dyn Future<Output = AgentResult<()>> + Send + 'static>>;

/// Parameters for starting or continuing one logical Agent Turn.
///
/// A continuation carries its Provider reference; the Worker restores the local
/// checkpoint before invoking the underlying Harness.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AgentRunParams {
    pub subagent_limit: Option<std::num::NonZeroU32>,
    pub command: Option<AgentCommandIdentity>,
    pub automation_origin: Option<talos_types::AutomationTurnOrigin>,
    pub context: AgentContext,
    pub input: AgentMessage,
    pub checkpoint: Option<AgentRunCheckpoint>,
    pub continuation: Option<AgentContinuationRef>,
}

impl AgentRunParams {
    /// Validate boundary invariants before Harness admission.
    pub fn validate(&self) -> AgentResult<()> {
        if self.input.turn_id != self.context.turn_id {
            return Err(TalosError::new(
                talos_error::ErrorCode::InvalidRequest,
                "Agent input turn_id does not match AgentContext identity",
            ));
        }
        if self.input.role != AgentMessageRole::User
            || self.input.status != AgentMessageStatus::Completed
        {
            return Err(TalosError::new(
                talos_error::ErrorCode::InvalidRequest,
                "Agent run input must be one completed User message",
            ));
        }
        if self.checkpoint.is_some() && self.continuation.is_none() {
            return Err(TalosError::new(
                talos_error::ErrorCode::InvalidRequest,
                "Agent checkpoint requires a Provider continuation reference",
            ));
        }
        Ok(())
    }
}

/// Parameters for delivering accepted input to one exact live invocation.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AgentInputParams {
    pub command: Option<AgentCommandIdentity>,
    pub agent_id: String,
    pub identity: AgentTurnRef,
    pub input: AgentMessage,
}

/// Parameters for interrupting one exact live invocation.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AgentInterruptParams {
    pub agent_id: String,
    pub identity: AgentTurnRef,
}

/// Harness admission facts that Service must persist before causing an effect.
pub struct AgentAdmission {
    pub events: Vec<AgentEvent>,
    pub permit: AgentAdmissionPermit,
}

/// One paused execution returned immediately after Harness admission.
pub struct AgentRunExecution {
    pub admission: AgentAdmission,
    pub events: AgentProgressEventReceiver,
    pub settlement: AgentRunSettlementReceiver,
}

/// Opaque one-shot capability released after admission facts become durable.
pub struct AgentAdmissionPermit {
    action: Option<AgentPermitFuture>,
}

impl AgentAdmissionPermit {
    /// Create a paused effect capability owned by AgentHarness.
    pub fn new(action: impl Future<Output = AgentResult<()>> + Send + 'static) -> Self {
        Self {
            action: Some(Box::pin(action)),
        }
    }

    /// Consume the capability exactly once after admission is durable.
    pub async fn consume(mut self) -> AgentResult<()> {
        self.action
            .take()
            .expect("Agent admission permit is consumed by value")
            .await
    }
}

impl std::fmt::Debug for AgentAdmissionPermit {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("AgentAdmissionPermit")
    }
}

/// Ordered Provider progress events; waiting and terminal facts are excluded.
pub struct AgentProgressEventReceiver {
    receiver: mpsc::Receiver<AgentEvent>,
}

impl AgentProgressEventReceiver {
    /// Receive a bounded batch of already available progress without delaying live events.
    pub async fn recv_many(&mut self, events: &mut Vec<AgentEvent>, limit: usize) -> usize {
        self.receiver.recv_many(events, limit).await
    }

    /// Wrap the Harness-owned progress channel for delivery to Service.
    pub fn new(receiver: mpsc::Receiver<AgentEvent>) -> Self {
        Self { receiver }
    }

    /// Receive the next canonical progress fact.
    pub async fn recv(&mut self) -> Option<AgentEvent> {
        self.receiver.recv().await
    }
}

impl std::fmt::Debug for AgentProgressEventReceiver {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("AgentProgressEventReceiver")
    }
}

/// Unique waiting or terminal delivery of canonical Agent facts and invocation outcome.
/// Harness produces live results; Service recovery reuses the same shape from
/// a durable checkpoint without invoking a Provider or fabricating snapshots.
/// Metrics are cumulative for the logical Turn, including earlier invocations.
#[derive(Debug, Clone)]
pub enum AgentRunSettlement {
    WaitingForInput {
        event: AgentEvent,
        continuation: AgentContinuationRef,
        metrics: AgentRunMetrics,
    },
    Finished {
        event: AgentEvent,
        metrics: AgentRunMetrics,
        /// Original live outcome; never reconstruct diagnostics from the event.
        result: talos_error::TalosResult<()>,
    },
}

/// One-shot settlement receiver paired with one `AgentRunExecution`.
pub struct AgentRunSettlementReceiver {
    receiver: oneshot::Receiver<AgentResult<AgentRunSettlement>>,
}

impl AgentRunSettlementReceiver {
    /// Wrap the Harness-owned settlement channel for delivery to Service.
    pub fn new(receiver: oneshot::Receiver<AgentResult<AgentRunSettlement>>) -> Self {
        Self { receiver }
    }

    /// Await the unique invocation settlement.
    pub async fn recv(self) -> AgentResult<AgentRunSettlement> {
        self.receiver.await.map_err(|_| {
            TalosError::new(
                talos_error::ErrorCode::InternalError,
                "Agent Harness stopped before delivering settlement",
            )
        })?
    }
}

impl std::fmt::Debug for AgentRunSettlementReceiver {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("AgentRunSettlementReceiver")
    }
}

#[cfg(test)]
#[path = "run_tests.rs"]
mod tests;

/// Stable product command identity; never includes Store leases or SQL state.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AgentCommandIdentity {
    pub command: talos_types::IdempotentCommand,
    pub idempotency_key: String,
    pub request_id: String,
}

/// Local typed transport evidence: a Commit was sent but its admission result was lost.
/// Callers retain the original command identity and reconcile Worker facts before retry.
#[derive(Debug)]
pub struct AgentAdmissionUnconfirmed(TalosError);
impl AgentAdmissionUnconfirmed {
    pub fn wrap(error: TalosError) -> TalosError {
        TalosError::new(error.error_code(), error.to_api_error().message.clone())
            .with_source(Self(error))
    }
    pub fn contains(error: &TalosError) -> bool {
        let mut source = std::error::Error::source(error);
        while let Some(current) = source {
            if current.is::<Self>() {
                return true;
            }
            source = current.source();
        }
        false
    }
}
impl std::fmt::Display for AgentAdmissionUnconfirmed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Worker admission confirmation was lost after Commit.")
    }
}
impl std::error::Error for AgentAdmissionUnconfirmed {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.0)
    }
}
