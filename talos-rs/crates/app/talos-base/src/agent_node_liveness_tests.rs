use super::*;
use futures_util::StreamExt;
use pb::agent_coordinator_rpc_server::AgentCoordinatorRpc;
use std::{pin::Pin, time::Duration};

#[derive(Clone, Copy, PartialEq)]
pub(super) enum CoordinatorOutput {
    Live,
    SilentAfterReady,
    LegacySilentAfterReady,
}

pub(super) struct CoordinatorOutputRpc {
    pub(super) inner: super::super::super::super::coordinator_rpc::CoordinatorRpc,
    pub(super) output: CoordinatorOutput,
}

#[tonic::async_trait]
impl AgentCoordinatorRpc for CoordinatorOutputRpc {
    async fn invoke_active_command(
        &self,
        request: Request<pb::ActiveSessionCommandRequest>,
    ) -> Result<Response<pb::ActiveSessionCommandResponse>, Status> {
        self.inner.invoke_active_command(request).await
    }

    type ConnectStream = Pin<
        Box<dyn futures_util::Stream<Item = Result<pb::ControlToCoordinatorFrame, Status>> + Send>,
    >;

    async fn connect(
        &self,
        request: Request<tonic::Streaming<pb::CoordinatorToControlFrame>>,
    ) -> Result<Response<Self::ConnectStream>, Status> {
        let response = self.inner.connect(request).await?;
        let output = self.output;
        match output {
            CoordinatorOutput::Live => Ok(response),
            CoordinatorOutput::SilentAfterReady | CoordinatorOutput::LegacySilentAfterReady => {
                Ok(
                    response.map(move |stream| {
                        // Preserve the real first acknowledgement and keep the original
                        // receiver alive, but blackhole every later response body frame.
                        Box::pin(futures_util::stream::unfold(
                            (stream, false),
                            move |(mut stream, forwarded)| async move {
                                let mut frame = if forwarded {
                                    futures_util::future::pending().await
                                } else {
                                    stream.next().await?
                                };
                                if let (
                                    CoordinatorOutput::LegacySilentAfterReady,
                                    Ok(pb::ControlToCoordinatorFrame {
                                        body:
                                            Some(pb::control_to_coordinator_frame::Body::Connected(
                                                ready,
                                            )),
                                    }),
                                ) = (output, &mut frame)
                                {
                                    ready.stream_heartbeat = false;
                                }
                                Some((frame, (stream, true)))
                            },
                        )) as Self::ConnectStream
                    }),
                )
            }
        }
    }
}

#[tokio::test]
async fn silent_coordinator_stream_is_retired_while_same_channel_unary_heartbeats_succeed() {
    coordinator_output_scenario(CoordinatorOutput::SilentAfterReady).await;
}

#[tokio::test]
async fn coordinator_echo_and_legacy_ready_preserve_connections() {
    for output in [
        CoordinatorOutput::Live,
        CoordinatorOutput::LegacySilentAfterReady,
    ] {
        coordinator_output_scenario(output).await;
    }
}

async fn coordinator_output_scenario(output: CoordinatorOutput) {
    let fixture = Fixture::new(/* endpoint */ None).await;
    let lease = registered(
        fixture
            .control
            .register(fixture.registration(&["talos"]))
            .await
            .unwrap()
            .into_inner(),
    );
    fixture.heartbeat(lease.clone()).await;
    let ingress =
        Ingress::with_coordinator_output(fixture.control.clone(), Arc::new(TestAuth), output).await;
    let auth = pb::NodeAuth {
        node_id: fixture.credential.node_id.clone(),
        token: lease.clone(),
    };
    let (_worker_sender, mut worker) = ingress.connect(auth).await.unwrap();
    ready(&mut worker).await;
    let client = agent_client::AgentNodeClient::connect(&ingress.endpoint, ingress.trust.clone())
        .await
        .unwrap();
    let (connection, _) = client
        .connect_coordinator(
            "user-1",
            &uuid::Uuid::new_v4().to_string(),
            &fixture.credential.node_id,
        )
        .await
        .unwrap();
    for _ in 0..4 {
        tokio::time::pause();
        tokio::time::advance(Duration::from_secs(15)).await;
        tokio::time::resume();
        let heartbeat = client
            .heartbeat(agent_protocol::NodeRequest {
                auth: agent_protocol::NodeAuth {
                    node_id: fixture.credential.node_id.clone(),
                    token: lease.clone(),
                },
                params: agent_protocol::HeartbeatParams {
                    health: talos_types::NodeHealth::Healthy,
                },
            })
            .await
            .unwrap();
        assert!(!heartbeat.registration_required);
    }
    if output == CoordinatorOutput::SilentAfterReady {
        assert!(
            tokio::time::timeout(Duration::from_millis(500), connection.ended())
                .await
                .is_ok(),
            "A healthy unary channel must not keep a silent bidi stream admitted forever"
        );
    } else {
        assert!(
            !connection.is_closed(),
            "Matched stream echoes or legacy fallback must preserve the connection"
        );
    }
    connection.close();
    ingress.close().await;
}
