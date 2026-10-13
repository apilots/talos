//! Reverse calls use the original typed resource PBs and the existing carrier.
use agent_pb as pb;
use futures_util::{future::BoxFuture, Stream};
use std::pin::Pin;
use tonic::Status;

pub type AgentResourceRequests =
    Pin<Box<dyn Stream<Item = Result<pb::worker_rpc_call::Request, Status>> + Send>>;
pub type AgentResourceResponses =
    Pin<Box<dyn Stream<Item = Result<pb::worker_rpc_result::Response, Status>> + Send>>;

/// Management hosts supply their original resource handlers. The SDK verifies
/// that an execution callback belongs to a live invocation. Closed source-file
/// methods may run without a Turn only when serves_files is advertised.
/// Implementations must still authorize the persisted Session or original file scope.
/// No Worker credentials, endpoint choice or product persistence is owned here.
pub trait AgentResourceRpcHandler: Send + Sync {
    /// Advertise the original FileService source without granting additional permissions.
    fn serves_files(&self) -> bool {
        false
    }
    fn dispatch(
        &self,
        caller: pb::CoordinatorCallContext,
        requests: AgentResourceRequests,
    ) -> BoxFuture<'_, Result<AgentResourceResponses, Status>>;
}
