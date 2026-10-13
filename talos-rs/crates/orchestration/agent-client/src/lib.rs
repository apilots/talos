//! Concrete network clients behind the shared Agent contracts.

mod management;
mod node;
mod peer_files;
mod remote;
mod resource_dispatch;
mod resources;
pub use resource_dispatch::{
    AgentResourceRequests, AgentResourceResponses, AgentResourceRpcHandler,
};
mod runner;
mod tls;

pub use node::AgentNodeClient;
pub use remote::RemoteAgentClient;
pub use resources::AgentSessionDataClient;
pub use runner::{AgentNodeLeaseObserver, AgentNodeLeaseUpdate, AgentNodeRunner};
pub use tls::AgentTlsTrust;

mod connection;
mod coordinator_connection;
pub use coordinator_connection::serve_coordinator_connection;
mod connection_writer;
mod harness_transport;
mod response_window;
mod transport_stream;
mod worker_connection;
pub use connection::AgentExecutionConnection;

// The Worker SDK accepts this canonical adapter; implementation stays in talos-agent.
pub use talos_agent::AgentHarnessRpcAdapter;

/// Preserve structured business failures; classify bare proxy/HTTP2 errors at transport ingress.
fn decode_rpc_status(status: tonic::Status) -> talos_error::TalosError {
    if status.details().is_empty()
        && matches!(
            status.code(),
            tonic::Code::Unknown | tonic::Code::Internal | tonic::Code::Unavailable
        )
    {
        talos_error::TalosError::from_source(
            talos_error::ErrorCode::ServiceUnavailable,
            "Agent RPC transport is unavailable",
            status,
        )
    } else {
        agent_protocol::error_pb::decode_status(status)
    }
}

mod environment;
pub use environment::AgentEnvironmentClient;
mod coordinator_commands;
