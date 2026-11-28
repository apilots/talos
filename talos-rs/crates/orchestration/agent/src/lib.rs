//! Agent execution framework and Provider SPI.
//!
//! This crate owns the in-process Harness, Provider/Session contracts, execution
//! context, and restricted capabilities. Product DTOs remain in `talos-agent-api`.

mod attachment;
mod definition;
mod error;
mod event_admission;
mod event_handle;
mod event_sink;
mod harness;
mod history;
mod provider;
mod reducer;
mod rpc;
mod session_owner;

use std::sync::Arc;

use harness::Harness;

pub use agent_protocol::{
    AgentAdmission, AgentAdmissionPermit, AgentCapabilities, AgentContext, AgentFuture,
    AgentHarness, AgentInputParams, AgentInterruptParams, AgentProgressEventReceiver, AgentResult,
    AgentRunCheckpoint, AgentRunExecution, AgentRunIntent, AgentRunParams, AgentRunSettlement,
    AgentRunSettlementReceiver, AgentSessionContext, AgentUserActionKind, AgentUserContentKind,
};
pub use attachment::{AttachmentReader, ResolvedAttachment, TurnFileScope};
pub use definition::{AgentDefinition, AgentDefinitionLoader};
pub use error::AgentRunFailure;
pub use event_handle::{AgentMessageBlockHandle, AgentMessageHandle};
pub use event_sink::{AgentEventPipeline, AgentEventSink};
pub use history::{SessionHistoryReadParams, SessionHistoryReader, SessionHistoryWindow};
pub use provider::{
    AgentOperationControl, AgentProvider, AgentSession, SessionCompactionParams,
    SessionCompactionResult, SessionTurnInputDelivery, SessionTurnInputReceiver, SessionTurnParams,
    SessionTurnResult,
};
pub use reducer::AgentEventReducer;
pub use rpc::{AgentHarnessRpcAdapter, AgentInvocationResourceBinder};
pub use session_owner::{SessionHold, SessionLease, SessionOwner};

/// Compose one provider-neutral Agent Harness around a concrete Provider.
///
/// Callers receive only the stable `AgentHarness` contract; the generic host
/// remains private to this crate and must not leak Provider implementation
/// details into Coordinator or application assembly. The Worker must pass its same
/// journal to this core and `LocalExecutionHarness`: only that journal supplies
/// waiting checkpoints and Provider continuation state.
pub fn create_agent_harness<P>(
    agent_id: String,
    provider: P,
    execution_store: Arc<talos_store::execution::WorkerExecutionStore>,
) -> Arc<dyn AgentHarness>
where
    P: AgentProvider,
{
    Arc::new(Harness::new(agent_id, provider, execution_store))
}

mod local_execution;
pub use local_execution::{LocalExecutionHarness, LocalTerminalObserver};
