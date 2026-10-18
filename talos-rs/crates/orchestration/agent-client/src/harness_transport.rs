//! Direct gRPC and reverse Connect share the same typed Harness messages.
use super::{
    connection::{AgentExecutionConnection, RequestMode},
    transport_stream::RpcStream,
};
use agent_pb as pb;
use futures_util::StreamExt;
use std::{sync::Arc, time::Duration};
use tonic::{transport::Channel, Request, Response, Status};

type Call = pb::worker_rpc_call::Request;
type Reply = pb::worker_rpc_result::Response;
#[derive(Clone)]
pub(crate) enum HarnessTransport {
    Direct(Channel),
    Outbound(Arc<AgentExecutionConnection>),
}

macro_rules! streaming {
    ($method:ident, $request:ident, $response:ident, $call:ident, $reply:ident, $timeout:expr, $mode:ident) => {
        pub(crate) async fn $method(
            &self,
            request: impl tonic::IntoStreamingRequest<Message = pb::$request>,
        ) -> Result<Response<RpcStream<pb::$response>>, Status> {
            match self {
                Self::Direct(channel) => {
                    let reply =
                        pb::agent_harness_rpc_client::AgentHarnessRpcClient::new(channel.clone())
                            .$method(request)
                            .await?;
                    Ok(Response::new(RpcStream::new(reply.into_inner())))
                }
                Self::Outbound(connection) => {
                    let requests = request
                        .into_streaming_request()
                        .into_inner()
                        .map(|request| Call::$call(Box::new(request)));
                    let stream = connection
                        .invoke(requests, $timeout, RequestMode::$mode)
                        .await?;
                    Ok(Response::new(stream.map(|reply| match reply {
                        Reply::$reply(reply) => Ok(*reply),
                        _ => Err(malformed_response()),
                    })))
                }
            }
        }
    };
}
macro_rules! unary {
    ($method:ident, $request:ident, $response:ident, $call:ident, $reply:ident, $timeout:expr, $mode:ident) => {
        pub(crate) async fn $method(
            &self,
            request: Request<pb::$request>,
        ) -> Result<Response<pb::$response>, Status> {
            match self {
                Self::Direct(channel) => {
                    pb::agent_harness_rpc_client::AgentHarnessRpcClient::new(channel.clone())
                        .$method(request)
                        .await
                }
                Self::Outbound(connection) => {
                    let request = request.into_inner();
                    let timeout: Duration = ($timeout)(&request);
                    let mut stream = connection
                        .invoke(
                            tokio_stream::once(Call::$call(Box::new(request))),
                            u64::try_from(timeout.as_millis()).unwrap_or(u64::MAX),
                            RequestMode::$mode,
                        )
                        .await?;
                    let reply = stream.message().await?.ok_or_else(|| {
                        agent_protocol::error_pb::encode_status(&super::connection_writer::closed())
                    })?;
                    let Reply::$reply(reply) = reply else {
                        return Err(malformed_response());
                    };
                    if stream.message().await?.is_some() {
                        return Err(malformed_response());
                    }
                    Ok(Response::new(*reply))
                }
            }
        }
    };
}
impl HarnessTransport {
    unary!(
        set_registration_admission,
        SetRegistrationAdmissionRequest,
        EmptyResponse,
        SetRegistrationAdmission,
        Empty,
        |_: &pb::SetRegistrationAdmissionRequest| Duration::from_secs(10),
        ControlUnary
    );

    unary!(
        read_session_execution_state,
        ReadSessionExecutionStateRequest,
        SessionExecutionStateResponse,
        ReadSessionExecutionState,
        SessionExecutionState,
        |_: &pb::ReadSessionExecutionStateRequest| Duration::from_secs(10),
        Unary
    );
    unary!(
        commit_session_execution_state,
        CommitSessionExecutionStateRequest,
        EmptyResponse,
        CommitSessionExecutionState,
        Empty,
        |_: &pb::CommitSessionExecutionStateRequest| Duration::from_secs(10),
        ControlUnary
    );
    streaming!(start, StartRequest, RunResponse, Start, Run, 0, Streaming);
    streaming!(
        submit_input,
        SubmitInputRequest,
        InputResponse,
        SubmitInput,
        Input,
        0,
        ControlStreaming
    );
    streaming!(
        compact_session,
        CompactSessionRequest,
        RunResponse,
        CompactSession,
        Run,
        0,
        Streaming
    );
    streaming!(
        form_dream_memory,
        FormDreamMemoryRequest,
        FormDreamMemoryResponse,
        FormDreamMemory,
        DreamMemory,
        300_000,
        Streaming
    );
    unary!(
        interrupt,
        InterruptRequest,
        InterruptResponse,
        Interrupt,
        Interrupt,
        |_: &pb::InterruptRequest| Duration::from_secs(30),
        ControlUnary
    );
    unary!(
        clear_session,
        SessionRequest,
        EmptyResponse,
        ClearSession,
        Empty,
        |_: &pb::SessionRequest| Duration::from_secs(30),
        Unary
    );
    unary!(
        close_session,
        SessionRequest,
        EmptyResponse,
        CloseSession,
        Empty,
        |_: &pb::SessionRequest| Duration::from_secs(30),
        Unary
    );
    unary!(
        read_workspace_memory,
        SessionRequest,
        WorkspaceMemoryResponse,
        ReadWorkspaceMemory,
        WorkspaceMemory,
        |_: &pb::SessionRequest| Duration::from_secs(10),
        Unary
    );
    unary!(
        update_workspace_memory,
        UpdateWorkspaceMemoryRequest,
        WorkspaceMemoryResponse,
        UpdateWorkspaceMemory,
        WorkspaceMemory,
        |_: &pb::UpdateWorkspaceMemoryRequest| Duration::from_secs(10),
        Unary
    );
    unary!(
        shutdown,
        ShutdownRequest,
        EmptyResponse,
        Shutdown,
        Empty,
        |request: &pb::ShutdownRequest| Duration::from_millis(
            request
                .params
                .as_ref()
                .map_or(0, |params| params.timeout_ms)
        )
        .saturating_add(Duration::from_secs(30)),
        ControlUnary
    );
}

fn malformed_response() -> Status {
    agent_protocol::error_pb::encode_status(&talos_error::TalosError::internal(
        "Agent RPC response is malformed",
    ))
}
