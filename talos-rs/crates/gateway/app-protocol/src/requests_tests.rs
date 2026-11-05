use super::*;
use serde_json::json;
use std::str::FromStr;
use talos_agent_api::{SubmitTurnInput, SubmitTurnInputRequest};
use talos_types::{AgentTurnRef, QuestionAnswer, UserActionResponse};

#[test]
fn trajectory_capabilities_decodes_null_params_from_jsonrpc() {
    let wire: crate::JsonRpcRequest = serde_json::from_value(json!({
        "jsonrpc": "2.0", "id": 1, "method": "trajectory/capabilities", "params": null
    }))
    .unwrap();
    let request = ClientRequest::decode(&wire.method, wire.params).unwrap();
    assert_eq!(
        request.params,
        ClientRequestParams::TrajectoryCapabilities(None)
    );
}

#[test]
fn decodes_typed_request_params() {
    let request = ClientRequest::decode(
        "session/get",
        Some(json!({
            "sessionId": "s1"
        })),
    )
    .unwrap();
    assert!(matches!(
        request.params,
        ClientRequestParams::SessionGet(SessionGetRequest { .. })
    ));
}

#[test]
fn session_patch_rejects_immutable_binding_fields() {
    for field in ["userId", "workspaceId", "agentId", "projectId"] {
        let mut params = json!({ "sessionId": "s1" });
        params[field] = json!("forbidden");
        let error = ClientRequest::decode("session/patch", Some(params))
            .expect_err("immutable Session bindings must be rejected");

        assert!(error.to_string().contains("unknown field"));
    }
}

#[test]
fn decodes_canonical_turn_input() {
    let request = ClientRequest::decode(
        "turn/input",
        Some(json!({
            "identity": { "sessionId": "s1", "turnId": "turn-1" },
            "input": {
                "type": "userActionResponse",
                "actionId": "action-1",
                "response": {
                    "type": "question",
                    "response": { "type": "selectedOption", "optionId": "continue" }
                }
            },
            "clientRequestId": null
        })),
    )
    .expect("Turn input should decode");

    assert_eq!(
        request.params,
        ClientRequestParams::TurnInput(SubmitTurnInputRequest {
            identity: AgentTurnRef::new("s1", "turn-1"),
            input: SubmitTurnInput::UserActionResponse {
                action_id: "action-1".to_string(),
                response: UserActionResponse::Question(QuestionAnswer::SelectedOption {
                    option_id: "continue".to_string(),
                }),
            },
            client_request_id: None,
        })
    );
}

#[test]
fn decodes_profile_read_params() {
    let request = ClientRequest::decode("profile/read", Some(json!({})))
        .expect("profile read request should decode");
    assert!(matches!(
        request.params,
        ClientRequestParams::ProfileRead(ProfileReadRequest {})
    ));
}

#[test]
fn skills_methods_parse_canonical_paths() {
    assert_eq!(
        ClientMethod::from_str("skills/list").unwrap(),
        ClientMethod::SkillsList
    );
    assert_eq!(
        ClientMethod::from_str("skills/refresh").unwrap(),
        ClientMethod::SkillsRefresh
    );
    assert_eq!(
        ClientMethod::from_str("skills/suggest").unwrap(),
        ClientMethod::SkillsSuggest
    );
    let request = ClientRequest::decode(
        "skills/resolve",
        Some(json!({
            "text": "Use $review",
            "selected": [{ "name": "review", "path": "/tmp/review/SKILL.md" }]
        })),
    )
    .expect("skills resolve request should decode");
    assert!(matches!(
        request.params,
        ClientRequestParams::SkillsResolve(SkillsResolveRequest { node_id: None, .. })
    ));

    let request = ClientRequest::decode("skills/refresh", None)
        .expect("skills refresh request should decode");
    assert!(matches!(
        request.params,
        ClientRequestParams::SkillsRefresh(SkillsRefreshRequest { node_id: None })
    ));
}

#[test]
fn client_method_parser_accepts_current_methods() {
    assert_eq!(
        ClientMethod::from_str("event/unsubscribe").unwrap(),
        ClientMethod::EventUnsubscribe
    );
    assert_eq!(
        ClientMethod::from_str("turn/input").unwrap(),
        ClientMethod::TurnInput
    );
    assert_eq!(
        ClientMethod::from_str("session/close").unwrap(),
        ClientMethod::SessionClose
    );
    assert!(ClientMethod::from_str("unknown.method").is_err());
}

#[test]
fn file_methods_decode_shared_requests_and_upload_envelope() {
    assert_eq!(
        ClientMethod::from_str("file/upload").unwrap(),
        ClientMethod::FileUpload
    );
    assert_eq!(
        ClientMethod::from_str("file/list").unwrap(),
        ClientMethod::FileList
    );
    assert_eq!(
        ClientMethod::from_str("file/read").unwrap(),
        ClientMethod::FileRead
    );
    assert_eq!(
        ClientMethod::from_str("file/update").unwrap(),
        ClientMethod::FileUpdate
    );
    assert_eq!(
        ClientMethod::from_str("file/delete").unwrap(),
        ClientMethod::FileDelete
    );

    let upload = ClientRequest::decode(
        "file/upload",
        Some(json!({
            "request": {
                "filename": "diagram.png",
                "scope": { "type": "personal" },
                "tags": ["diagram"],
                "metadata": {}
            },
            "dataBase64": "iVBORw=="
        })),
    )
    .expect("file upload should decode");
    assert!(matches!(
        upload.params,
        ClientRequestParams::FileUpload(FileUploadParams { .. })
    ));

    let list = ClientRequest::decode(
        "file/list",
        Some(json!({
            "limit": 20,
            "scope": { "type": "project", "projectId": "project-1" },
            "tags": ["diagram"]
        })),
    )
    .expect("file list should decode");
    assert!(matches!(
        list.params,
        ClientRequestParams::FileList(talos_types::FileListRequest { .. })
    ));
}

#[test]
fn message_queue_methods_decode_shared_camel_case_requests() {
    let cases = [
        (
            "messageQueue/add",
            json!({
                "sessionId": "s1",
                "input": { "content": [{ "type": "text", "text": "first" }] }
            }),
        ),
        ("messageQueue/list", json!({ "sessionId": "s1" })),
        (
            "messageQueue/update",
            json!({
                "sessionId": "s1",
                "messageId": "q1",
                "input": { "content": [{ "type": "text", "text": "updated" }] }
            }),
        ),
        (
            "messageQueue/delete",
            json!({ "sessionId": "s1", "messageId": "q1" }),
        ),
        (
            "messageQueue/reorder",
            json!({ "sessionId": "s1", "messageIds": ["q2", "q1"] }),
        ),
        (
            "messageQueue/start",
            json!({ "sessionId": "s1", "messageId": null }),
        ),
    ];

    for (method, params) in cases {
        ClientRequest::decode(method, Some(params))
            .unwrap_or_else(|error| panic!("{method} should decode: {error}"));
    }

    let start = ClientRequest::decode("messageQueue/start", Some(json!({ "sessionId": "s1" })))
        .expect("omitted messageId should decode");
    assert_eq!(
        start.params,
        ClientRequestParams::MessageQueueStart(MessageQueueStartRequest {
            session_id: "s1".to_string(),
            message_id: None,
        })
    );
}

#[test]
fn automation_public_methods_decode_closed_shared_contracts() {
    let cases = [
        ("automation/read", json!({"sessionId":"s1"})),
        (
            "automation/execute",
            json!({"sessionId":"s1", "action":{"target":"goal", "action":{"type":"pause", "goalId":"g1"}}}),
        ),
        (
            "automation/list",
            json!({"sessionId":null,"scope":"unfinished","cursor":null,"limit":20}),
        ),
        ("automation/commandCatalog", json!({})),
        (
            "automation/resolveCommand",
            json!({"text":"/loop 5m check CI"}),
        ),
        ("builtinJob/list", json!({"computerId":"c1"})),
        (
            "builtinJob/control",
            json!({"computerId":"c1","executionInstanceId":"i1","jobId":"profileUsageRetention","action":"pause"}),
        ),
    ];
    for (method, params) in cases {
        let method_id = ClientMethod::from_str(method).expect("canonical public method");
        assert_eq!(method_id.as_str(), method);
        let request = ClientRequest::decode(method, Some(params.clone())).unwrap();
        let serialized = serde_json::to_value(request.params).unwrap();
        assert_eq!(serialized["method"], method);
        assert_eq!(serialized["params"], params);
    }
    for params in [
        json!({"sessionId":"s1","action":{"target":"builtinJob","action":{"type":"pause"}}}),
        json!({"sessionId":"s1","action":{"target":"loop","action":{"type":"runCommand","command":"anything"}}}),
    ] {
        assert!(ClientRequest::decode("automation/execute", Some(params)).is_err());
    }
}

#[test]
fn automation_notification_is_a_session_invalidation_without_runtime_identity() {
    let change =
        crate::ServerNotification::AutomationChanged(talos_types::AutomationChangedNotification {
            session_id: "s1".into(),
        });
    assert_eq!(
        serde_json::to_value(change).unwrap(),
        json!({
            "method":"automation/changed", "params":{"sessionId":"s1"}
        })
    );
    let request = talos_types::AutomationRequest {
        session_id: "s1".into(),
        action: talos_types::AutomationAction::Goal {
            action: talos_types::GoalAction::Read {},
        },
    };
    let fields = serde_json::to_value(request).unwrap();
    for field in ["caller", "turnId", "callId", "agentId"] {
        assert!(fields.get(field).is_none());
    }
}
