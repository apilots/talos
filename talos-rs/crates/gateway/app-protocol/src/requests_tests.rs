use super::*;
use serde_json::json;
use talos_types::talos::TalosInputItem;
use talos_types::{QuestionAnswer, SubmitUserActionRequest, UserActionResponse};

#[test]
fn method_parses_dot_name() {
    assert_eq!(
        ClientMethod::from_str("turn.start").unwrap(),
        ClientMethod::TurnStart
    );
}

#[test]
fn decodes_typed_request_params() {
    let request = ClientRequest::decode(
        "session.get",
        Some(json!({
            "session_id": "s1"
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
    for field in ["user_id", "workspace_id", "agent_id", "project_id"] {
        let mut params = json!({ "session_id": "s1" });
        params[field] = json!("forbidden");
        let error = ClientRequest::decode("session.patch", Some(params))
            .expect_err("immutable Session bindings must be rejected");

        assert!(error.to_string().contains("unknown field"));
    }
}

#[test]
fn decodes_canonical_user_action_submission() {
    let request = ClientRequest::decode(
        "userAction/submit",
        Some(json!({
            "sessionId": "s1",
            "turnId": "turn-1",
            "actionId": "action-1",
            "response": {
                "type": "question",
                "response": { "type": "selectedOption", "optionId": "continue" }
            }
        })),
    )
    .expect("user action submission should decode");

    assert_eq!(
        request.params,
        ClientRequestParams::UserActionSubmit(SubmitUserActionRequest {
            session_id: "s1".to_string(),
            turn_id: "turn-1".to_string(),
            action_id: "action-1".to_string(),
            response: UserActionResponse::Question(QuestionAnswer::SelectedOption {
                option_id: "continue".to_string(),
            }),
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
fn turn_start_input_uses_shared_talos_input_item() {
    let value = json!({
        "type": "skill_selection",
        "name": "review",
        "path": "/tmp/review/SKILL.md"
    });

    let item: TalosInputItem =
        serde_json::from_value(value).expect("skill selection should decode");

    assert_eq!(
        item,
        TalosInputItem::SkillSelection {
            name: "review".to_string(),
            path: "/tmp/review/SKILL.md".to_string(),
        }
    );
}

#[test]
fn skills_methods_parse_without_legacy_skill_list_alias() {
    assert_eq!(
        ClientMethod::from_str("skills.list").unwrap(),
        ClientMethod::SkillsList
    );
    assert_eq!(
        ClientMethod::from_str("skills.refresh").unwrap(),
        ClientMethod::SkillsRefresh
    );
    assert_eq!(
        ClientMethod::from_str("skills.suggest").unwrap(),
        ClientMethod::SkillsSuggest
    );
    assert!(ClientMethod::from_str("skill.list").is_err());

    let request = ClientRequest::decode(
        "skills.resolve",
        Some(json!({
            "text": "Use $review",
            "selected": [{ "name": "review", "path": "/tmp/review/SKILL.md" }]
        })),
    )
    .expect("skills resolve request should decode");
    assert!(matches!(
        request.params,
        ClientRequestParams::SkillsResolve(SkillsResolveRequest { .. })
    ));

    let request = ClientRequest::decode("skills.refresh", None)
        .expect("skills refresh request should decode");
    assert!(matches!(
        request.params,
        ClientRequestParams::SkillsRefresh(SkillsRefreshRequest {})
    ));
}

#[test]
fn client_method_parser_accepts_current_methods() {
    assert!(DEFERRED_CLIENT_METHODS.is_empty());
    assert_eq!(
        ClientMethod::from_str("event.unsubscribe").unwrap(),
        ClientMethod::EventUnsubscribe
    );
    assert_eq!(
        ClientMethod::from_str("tool.dynamic_result.submit").unwrap(),
        ClientMethod::ToolDynamicResultSubmit
    );
    assert_eq!(
        ClientMethod::from_str("userAction/submit").unwrap(),
        ClientMethod::UserActionSubmit
    );
    assert_eq!(
        ClientMethod::from_str("session.close").unwrap(),
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
                "input": [{ "type": "text", "text": "first" }]
            }),
        ),
        ("messageQueue/list", json!({ "sessionId": "s1" })),
        (
            "messageQueue/update",
            json!({
                "sessionId": "s1",
                "messageId": "q1",
                "input": [{ "type": "text", "text": "updated" }]
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
        (
            "messageQueue/steer",
            json!({ "sessionId": "s1", "messageId": "q1", "turnId": "t1" }),
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
