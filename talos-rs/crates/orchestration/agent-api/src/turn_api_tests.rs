use serde_json::json;
use talos_types::{QuestionAnswer, UserActionResponse};

use super::{SubmitTurnInput, SubmitTurnInputRequest};
use crate::AgentTurnRef;

#[test]
fn turn_input_serializes_as_one_camel_case_contract() {
    let request = SubmitTurnInputRequest {
        identity: AgentTurnRef::new("session-1", "turn-1"),
        input: SubmitTurnInput::UserActionResponse {
            action_id: "action-1".to_owned(),
            response: UserActionResponse::Question(QuestionAnswer::FreeForm {
                text: "continue".to_owned(),
            }),
        },
        client_request_id: Some("request-1".to_owned()),
    };

    assert_eq!(
        serde_json::to_value(request).unwrap(),
        json!({
            "identity": { "sessionId": "session-1", "turnId": "turn-1" },
            "input": {
                "type": "userActionResponse",
                "actionId": "action-1",
                "response": {
                    "type": "question",
                    "response": { "type": "freeForm", "text": "continue" }
                }
            },
            "clientRequestId": "request-1"
        })
    );
}
