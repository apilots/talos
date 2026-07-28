use std::future::Future;
use std::pin::Pin;

use talos_error::TalosResult;
use talos_types::{
    AgentTreeSnapshot, FollowUpSubagentRequest, InterruptSubagentRequest, ListSubagentsRequest,
    SendMessageResult, SendSubagentMessageRequest, SpawnSubagentRequest, SubagentSnapshot,
    SubagentSpawnResult, WaitSubagentRequest,
};

/// Boxed operation future used to keep `AgentControl` object-safe.
pub type AgentControlFuture<'a, T> = Pin<Box<dyn Future<Output = TalosResult<T>> + Send + 'a>>;

/// Stable Tool-facing port for Agent-tree control.
///
/// Implementations must delegate to the owning application service. Tools use
/// this port only for typed control operations; it must not own execution,
/// lifecycle state, routing, or event transport.
pub trait AgentControl: Send + Sync {
    fn spawn(&self, request: SpawnSubagentRequest) -> AgentControlFuture<'_, SubagentSpawnResult>;

    fn send_message(
        &self,
        request: SendSubagentMessageRequest,
    ) -> AgentControlFuture<'_, SendMessageResult>;

    fn wait(&self, request: WaitSubagentRequest) -> AgentControlFuture<'_, SubagentSnapshot>;

    fn interrupt(
        &self,
        request: InterruptSubagentRequest,
    ) -> AgentControlFuture<'_, SubagentSnapshot>;

    fn follow_up(
        &self,
        request: FollowUpSubagentRequest,
    ) -> AgentControlFuture<'_, SubagentSnapshot>;

    fn list(&self, request: ListSubagentsRequest) -> AgentControlFuture<'_, AgentTreeSnapshot>;
}
