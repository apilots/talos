//! Protected loopback callback listener for the same management resource owners.
use crate::agent_node_rpc::{NodeSessionDataRpc, NodeSubagentControlRpc, NodeToolImageRpc};
use agent_client::AgentSessionDataClient;
use agent_protocol::NodeAuth;
use std::{sync::Arc, time::Duration};
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_tools::AgentControl;
use tokio::{net::TcpListener, sync::Mutex, task::JoinHandle};
use tokio_stream::wrappers::TcpListenerStream;
use tokio_util::sync::CancellationToken;

pub(crate) struct LocalResourceRpc {
    stop: CancellationToken,
    task: Mutex<Option<JoinHandle<TalosResult<()>>>>,
}
impl LocalResourceRpc {
    pub(crate) fn cancellation_token(&self) -> CancellationToken {
        self.stop.clone()
    }

    pub(crate) async fn start(
        auth: NodeAuth,
        data: NodeSessionDataRpc,
        images: NodeToolImageRpc,
        control: Arc<dyn AgentControl>,
    ) -> TalosResult<(Arc<Self>, AgentSessionDataClient)> {
        let listener = TcpListener::bind("127.0.0.1:0").await.map_err(|error| {
            TalosError::from_source(
                ErrorCode::ServiceUnavailable,
                "Local resource RPC listener is unavailable",
                error,
            )
        })?;
        let address = listener.local_addr().map_err(|error| {
            TalosError::internal_with_source(
                "Read local resource RPC address; technical details were withheld.",
                error,
            )
        })?;
        let data = data.with_host_auth(auth.clone());
        let images = images.with_authorization(data.clone());
        let control = NodeSubagentControlRpc::new(data.clone(), control);
        let stop = CancellationToken::new();
        let task_stop = stop.clone();
        let task = tokio::spawn(async move {
            tonic::transport::Server::builder()
                .add_service(
                    agent_pb::agent_session_data_rpc_server::AgentSessionDataRpcServer::new(data),
                )
                .add_service(
                    agent_pb::agent_subagent_control_rpc_server::AgentSubagentControlRpcServer::new(
                        control,
                    ),
                )
                .add_service(
                    agent_pb::agent_tool_image_rpc_server::AgentToolImageRpcServer::new(images),
                )
                .serve_with_incoming_shutdown(
                    TcpListenerStream::new(listener),
                    task_stop.cancelled_owned(),
                )
                .await
                .map_err(|error| {
                    TalosError::internal_with_source(
                        "Serve local resource RPC; technical details were withheld.",
                        error,
                    )
                })
        });
        let host = Arc::new(Self {
            stop,
            task: Mutex::new(Some(task)),
        });
        let client = AgentSessionDataClient::connect_loopback(address, auth).await?;
        Ok((host, client))
    }
    pub(crate) async fn shutdown(&self) -> TalosResult<()> {
        self.stop.cancel();
        let Some(mut task) = self.task.lock().await.take() else {
            return Ok(());
        };
        match tokio::time::timeout(Duration::from_secs(5), &mut task).await {
            Ok(result) => result.map_err(|error| {
                TalosError::internal_with_source(
                    "Local resource RPC task failed; technical details were withheld.",
                    error,
                )
            })?,
            Err(error) => {
                task.abort();
                if let Err(joined) = task.await {
                    if !joined.is_cancelled() {
                        return Err(TalosError::internal_with_source(
                            "Local resource RPC task failed; technical details were withheld.",
                            joined,
                        ));
                    }
                }
                Err(TalosError::from_source(
                    ErrorCode::Timeout,
                    "Local resource RPC drain timed out",
                    error,
                ))
            }
        }
    }
}
impl Drop for LocalResourceRpc {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}
