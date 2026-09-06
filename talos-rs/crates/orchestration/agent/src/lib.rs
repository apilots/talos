//! Agent execution framework and Provider SPI.
//!
//! This crate owns the in-process Harness, Provider/Session contracts, execution
//! context, and restricted capabilities. Product DTOs remain in `talos-agent-api`.

mod attachment;
mod capability;
mod context;
mod definition;
mod error;
mod event_admission;
mod event_handle;
mod event_sink;
mod harness;
mod history;
mod port;
mod provider;
mod reducer;
mod run;

use std::sync::Arc;

use harness::Harness;

pub use attachment::{AttachmentReader, ResolvedAttachment, TurnFileScope};
pub use capability::{AgentCapabilities, AgentUserActionKind, AgentUserContentKind};
pub use context::{AgentContext, AgentSessionContext};
pub use definition::{AgentDefinition, AgentDefinitionLoader};
pub use error::{AgentResult, AgentRunFailure};
pub use event_handle::{AgentMessageBlockHandle, AgentMessageHandle};
pub use event_sink::{AgentEventPipeline, AgentEventSink};
pub use history::{SessionHistoryReadParams, SessionHistoryReader, SessionHistoryWindow};
pub use port::{AgentFuture, AgentHarness};
pub use provider::{
    AgentOperationControl, AgentProvider, AgentSession, SessionCompactionParams,
    SessionCompactionResult, SessionTurnInputDelivery, SessionTurnInputReceiver, SessionTurnParams,
    SessionTurnResult,
};
pub use reducer::{AgentEventReducer, AgentRunCheckpoint};
pub use run::{
    AgentAdmission, AgentAdmissionPermit, AgentInputParams, AgentInterruptParams,
    AgentProgressEventReceiver, AgentRunExecution, AgentRunParams, AgentRunSettlement,
    AgentRunSettlementReceiver,
};

/// Compose one provider-neutral Agent Harness around a concrete Provider.
///
/// Callers receive only the stable `AgentHarness` contract; the generic host
/// remains private to this crate and must not leak Provider implementation
/// details into Coordinator or application assembly.
pub fn create_agent_harness<P>(agent_id: String, provider: P) -> Arc<dyn AgentHarness>
where
    P: AgentProvider,
{
    Arc::new(Harness::new(agent_id, provider))
}
