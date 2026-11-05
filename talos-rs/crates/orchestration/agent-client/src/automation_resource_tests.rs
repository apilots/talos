use std::sync::{Arc, Mutex};

use agent_protocol::NodeAuth;
use talos_types::{
    AgentCaller, AutomationAction, AutomationEffect, AutomationTurnOrigin, LoopAction,
    SessionAutomationSnapshot,
};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;
use tonic::{Request, Response, Status};

use super::*;

#[derive(Clone)]
struct MockControl {
    seen: Arc<Mutex<Vec<pb::ExecuteAutomationRequest>>>,
    reply: pb::ExecuteAutomationResponse,
}

#[tonic::async_trait]
impl pb::agent_automation_control_rpc_server::AgentAutomationControlRpc for MockControl {
    async fn execute_automation(
        &self,
        request: Request<pb::ExecuteAutomationRequest>,
    ) -> Result<Response<pb::ExecuteAutomationResponse>, Status> {
        self.seen.lock().unwrap().push(request.into_inner());
        Ok(Response::new(self.reply.clone()))
    }
}

struct Fixture {
    client: AgentSessionDataClient,
    seen: Arc<Mutex<Vec<pb::ExecuteAutomationRequest>>>,
    stop: CancellationToken,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.stop.cancel();
        self.task.abort();
    }
}

async fn fixture(reply: pb::ExecuteAutomationResponse) -> Fixture {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let service = MockControl {
        seen: seen.clone(),
        reply,
    };
    let stop = CancellationToken::new();
    let shutdown = stop.clone();
    let task = tokio::spawn(async move {
        let incoming = futures_util::stream::unfold(listener, |listener| async move {
            let next = listener.accept().await.map(|(stream, _)| stream);
            Some((next, listener))
        });
        tonic::transport::Server::builder()
            .add_service(
                pb::agent_automation_control_rpc_server::AgentAutomationControlRpcServer::new(
                    service,
                ),
            )
            .serve_with_incoming_shutdown(incoming, shutdown.cancelled_owned())
            .await
            .unwrap();
    });
    let mut client = AgentSessionDataClient::connect_loopback(
        address,
        NodeAuth {
            node_id: "node".into(),
            token: "lease-token".into(),
        },
    )
    .await
    .unwrap();
    client.caller = Some(pb::CoordinatorCallContext {
        connection_id: "connection".into(),
        rpc_call_id: 31,
        coordinator_id: "coordinator".into(),
        session: Some(pb::SessionContext {
            session_id: "session".into(),
            agent_id: "agent".into(),
            user_id: "workspace-user".into(),
            workspace_id: "workspace".into(),
        }),
        turn_id: Some("turn".into()),
        project_id: None,
    });
    Fixture {
        client,
        seen,
        stop,
        task,
    }
}

fn request() -> AutomationModelRequest {
    AutomationModelRequest {
        caller: AgentCaller {
            session_id: "session".into(),
            turn_id: "turn".into(),
            agent_id: "agent".into(),
        },
        call_id: "runtime-call".into(),
        action: AutomationAction::Loop {
            action: LoopAction::List {},
        },
    }
}

fn result() -> AutomationControlResult {
    AutomationControlResult {
        created_object: None,
        effect: AutomationEffect::PendingSettlement,
        bound_origin: Some(AutomationTurnOrigin::Loop {
            loop_id: "loop".into(),
            occurrence_id: "occurrence".into(),
            config_revision: 7,
        }),
        snapshot: SessionAutomationSnapshot {
            session_id: "session".into(),
            goal: None,
            loops: Vec::new(),
        },
    }
}

#[tokio::test]
async fn actual_loopback_rpc_preserves_authenticated_scope_runtime_call_and_binding() {
    let expected = result();
    let fixture = fixture(automation_pb::encode_response(Ok(expected.clone()))).await;
    assert_eq!(
        AutomationControl::execute(&fixture.client, request())
            .await
            .unwrap(),
        expected
    );
    let seen = fixture.seen.lock().unwrap();
    assert_eq!(seen.len(), 1);
    assert_eq!(
        automation_pb::decode_model_params(seen[0].params.clone()).unwrap(),
        request()
    );
    assert_eq!(seen[0].caller, fixture.client.caller);
    assert_eq!(
        seen[0].auth,
        Some(node_pb::encode_auth(fixture.client.current_auth()))
    );
}

#[tokio::test]
async fn typed_service_failure_crosses_rpc_without_success_or_fallback() {
    let expected = TalosError::new(ErrorCode::PermissionDenied, "Activation intent is missing");
    let fixture = fixture(automation_pb::encode_response(Err(expected.clone()))).await;
    let actual = AutomationControl::execute(&fixture.client, request())
        .await
        .unwrap_err();
    assert_eq!(actual.to_api_error(), expected.to_api_error());
    assert_eq!(fixture.seen.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn mismatched_or_empty_runtime_identity_is_rejected_before_rpc() {
    let fixture = fixture(automation_pb::encode_response(Ok(result()))).await;
    for mutate in [
        |request: &mut AutomationModelRequest| request.caller.session_id = "other".into(),
        |request: &mut AutomationModelRequest| request.caller.agent_id = "other".into(),
        |request: &mut AutomationModelRequest| request.caller.turn_id = "other".into(),
        |request: &mut AutomationModelRequest| request.call_id.clear(),
    ] {
        let mut request = request();
        mutate(&mut request);
        assert!(AutomationControl::execute(&fixture.client, request)
            .await
            .is_err());
    }
    assert!(fixture.seen.lock().unwrap().is_empty());
}

#[tokio::test]
async fn malformed_or_other_session_response_is_not_replaced_with_empty_success() {
    for reply in [
        pb::ExecuteAutomationResponse { result: None },
        automation_pb::encode_response(Ok(AutomationControlResult {
            created_object: None,
            snapshot: SessionAutomationSnapshot {
                session_id: "other".into(),
                goal: None,
                loops: Vec::new(),
            },
            ..result()
        })),
    ] {
        let fixture = fixture(reply).await;
        assert!(AutomationControl::execute(&fixture.client, request())
            .await
            .is_err());
        assert_eq!(fixture.seen.lock().unwrap().len(), 1);
    }
}
