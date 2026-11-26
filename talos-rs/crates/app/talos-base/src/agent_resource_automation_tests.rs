//! Exercise automation authorization at the reverse invocation boundary.
use super::*;
use agent_protocol::{automation_pb, harness_pb, node_pb, NodeAuth};
use pb::agent_automation_control_rpc_server::AgentAutomationControlRpc;
use talos_tools::{AutomationControl, AutomationControlFuture};
use talos_types::{
    AutomationAction, AutomationControlResult, AutomationEffect, AutomationModelRequest,
    LoopAction, SessionAutomationSnapshot,
};

#[derive(Default)]
struct RecordedAutomation(std::sync::Mutex<Vec<AutomationModelRequest>>);

impl AutomationControl for RecordedAutomation {
    fn execute(&self, request: AutomationModelRequest) -> AutomationControlFuture<'_> {
        let session_id = request.caller.session_id.clone();
        self.0.lock().unwrap().push(request);
        Box::pin(async move {
            Ok(AutomationControlResult {
                created_object: None,
                effect: AutomationEffect::Applied,
                bound_origin: None,
                snapshot: SessionAutomationSnapshot {
                    session_id,
                    goal: None,
                    loops: Vec::new(),
                },
            })
        })
    }
}

#[tokio::test]
async fn automation_requires_the_exact_reverse_invocation() {
    let storage = Arc::new(
        SqliteStorage::new_with_max_connections(":memory:", 1)
            .await
            .unwrap(),
    );
    talos_store::migration::run_migrations(
        storage.pool(),
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../conf/migrations/sqlite"),
    )
    .await
    .unwrap();
    let pool = storage.pool().clone();
    let nodes = AgentNodeService::new(Arc::new(DatabaseAgentNodeStore::new(&storage)));
    let sessions = Arc::new(DatabaseSessionStore::new(storage).await.unwrap());
    let record = session_record();
    sessions.insert_session_record(&record).await.unwrap();
    let data = NodeSessionDataRpc::new(
        nodes,
        sessions,
        Arc::new(EmptyHistory),
        Arc::new(TestAttachments),
        Arc::new(TestMemoryProgress::default()),
    );
    let auth = NodeAuth {
        node_id: record.execution_node_id.clone(),
        token: "foreign-lease".into(),
    };
    let control = Arc::new(RecordedAutomation::default());
    let session = agent_protocol::AgentSessionContext {
        agent_id: record.agent_id.clone(),
        session_id: record.id.clone(),
        workspace: talos_config::Config::default()
            .resolve_workspace("default", "user-1")
            .unwrap(),
    };
    let params = AutomationModelRequest {
        caller: talos_types::AgentCaller {
            session_id: record.id.clone(),
            agent_id: record.agent_id.clone(),
            turn_id: "turn-automation".into(),
        },
        call_id: "tool-call-automation".into(),
        action: AutomationAction::Loop {
            action: LoopAction::List {},
        },
    };
    let scope = pb::CoordinatorCallContext {
        coordinator_id: "original-coordinator".into(),
        connection_id: "original-connection".into(),
        rpc_call_id: 1,
        session: Some(harness_pb::encode_session_context(session)),
        turn_id: Some(params.caller.turn_id.clone()),
        project_id: None,
    };
    let reverse = NodeAutomationControlRpc::new(
        data.for_invocation(record.execution_node_id.clone(), scope.clone()),
        control.clone(),
    );
    for wrong_agent in [false, true] {
        let mut forged = params.clone();
        if wrong_agent {
            forged.caller.agent_id = "other-agent".into();
        } else {
            forged.caller.turn_id = "other-turn".into();
        }
        let response = reverse
            .execute_automation(Request::new(pb::ExecuteAutomationRequest {
                auth: Some(node_pb::encode_auth(auth.clone())),
                caller: Some(scope.clone()),
                params: Some(automation_pb::encode_model_params(forged).unwrap()),
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(automation_pb::decode_response(response)
            .unwrap_err()
            .is_code(ErrorCode::PermissionDenied));
    }
    let accepted = reverse
        .execute_automation(Request::new(pb::ExecuteAutomationRequest {
            auth: Some(node_pb::encode_auth(auth)),
            caller: Some(scope),
            params: Some(automation_pb::encode_model_params(params.clone()).unwrap()),
        }))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(
        automation_pb::decode_response(accepted).unwrap().snapshot,
        SessionAutomationSnapshot {
            session_id: record.id,
            goal: None,
            loops: Vec::new(),
        }
    );
    assert_eq!(*control.0.lock().unwrap(), vec![params]);
    drop(reverse);
    drop(data);
    pool.close().await;
}
