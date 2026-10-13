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
/// A continuation reuses the same API and must provide both the canonical
/// checkpoint and Provider-owned continuation reference.
#[derive(Debug, Clone, PartialEq)]
pub struct AgentRunParams {
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
        if self.checkpoint.is_some() != self.continuation.is_some() {
            return Err(TalosError::new(
                talos_error::ErrorCode::InvalidRequest,
                "Agent continuation requires both checkpoint and Provider reference",
            ));
        }
        Ok(())
    }
}

/// Parameters for delivering accepted input to one exact live invocation.
#[derive(Debug, Clone, PartialEq)]
pub struct AgentInputParams {
    pub agent_id: String,
    pub identity: AgentTurnRef,
    pub input: AgentMessage,
}

/// Parameters for interrupting one exact live invocation.
#[derive(Debug, Clone, PartialEq)]
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

/// Unique waiting or terminal delivery of canonical Agent facts and reducer state.
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
        messages: Vec<AgentMessage>,
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
