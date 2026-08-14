use super::*;
use talos_types::{QuestionAnswer, UserActionResponse};

fn arguments() -> Value {
    serde_json::json!({
        "header": "Execution Style",
        "question": "How should the agent proceed?",
        "options": [
            { "id": "review", "label": "Review", "description": "Review context first" },
            { "id": "direct", "label": "Direct", "description": "Act now" }
        ]
    })
}

fn context(response: Option<UserActionResponse>) -> ToolContext {
    let mut context = ToolContext::new("session-1", "user-1");
    context.bind_current_call_id("call-1");
    if let Some(response) = response {
        context.set_user_action_response("call-1", response);
    }
    context
}

#[tokio::test]
async fn initial_call_returns_typed_question_pending() {
    let outcome = AskUserQuestionTool
        .execute_outcome(arguments(), context(None))
        .await
        .expect("question should become pending");

    assert_eq!(
        outcome,
        ToolDispatchOutcome::pending_question(
            "AskUserQuestion",
            QuestionParams {
                header: "Execution Style".to_string(),
                question: "How should the agent proceed?".to_string(),
                options: vec![
                    QuestionOption {
                        id: "review".to_string(),
                        label: "Review".to_string(),
                        description: "Review context first".to_string(),
                    },
                    QuestionOption {
                        id: "direct".to_string(),
                        label: "Direct".to_string(),
                        description: "Act now".to_string(),
                    },
                ],
            },
        )
    );
}

#[tokio::test]
async fn selected_option_resumes_with_stable_tool_result() {
    let outcome = AskUserQuestionTool
        .execute_outcome(
            arguments(),
            context(Some(UserActionResponse::Question(
                QuestionAnswer::SelectedOption {
                    option_id: "review".to_string(),
                },
            ))),
        )
        .await
        .expect("selected option should resume");

    let ToolOutputPayload::Function {
        structured_content, ..
    } = outcome.payload().expect("resumed question should execute")
    else {
        panic!("expected function payload");
    };
    assert_eq!(
        structured_content,
        &Some(serde_json::json!({
            "question": "How should the agent proceed?",
            "answer": {
                "type": "selectedOption",
                "optionId": "review",
                "label": "Review"
            }
        }))
    );
}

#[tokio::test]
async fn free_form_resumes_with_trimmed_tool_result() {
    let outcome = AskUserQuestionTool
        .execute_outcome(
            arguments(),
            context(Some(UserActionResponse::Question(
                QuestionAnswer::FreeForm {
                    text: "  Use a staged rollout  ".to_string(),
                },
            ))),
        )
        .await
        .expect("free-form answer should resume");

    let ToolOutputPayload::Function {
        structured_content, ..
    } = outcome.payload().expect("resumed question should execute")
    else {
        panic!("expected function payload");
    };
    assert_eq!(
        structured_content,
        &Some(serde_json::json!({
            "question": "How should the agent proceed?",
            "answer": {
                "type": "freeForm",
                "text": "Use a staged rollout"
            }
        }))
    );
}

#[tokio::test]
async fn invalid_question_and_answer_fail_closed() {
    let duplicate_options = serde_json::json!({
        "header": "Mode",
        "question": "Choose",
        "options": [
            { "id": "same", "label": "One", "description": "First" },
            { "id": "same", "label": "Two", "description": "Second" }
        ]
    });
    assert!(AskUserQuestionTool
        .execute_outcome(duplicate_options, context(None))
        .await
        .is_err());

    assert!(AskUserQuestionTool
        .execute_outcome(
            arguments(),
            context(Some(UserActionResponse::Question(
                QuestionAnswer::SelectedOption {
                    option_id: "missing".to_string(),
                },
            ))),
        )
        .await
        .is_err());

    assert!(AskUserQuestionTool
        .execute_outcome(
            arguments(),
            context(Some(UserActionResponse::Question(
                QuestionAnswer::FreeForm {
                    text: "   ".to_string(),
                },
            ))),
        )
        .await
        .is_err());
}

#[test]
fn schema_has_fixed_question_shape_without_free_form_switch() {
    let schema = AskUserQuestionTool
        .parameters_schema()
        .expect("question schema should exist");
    assert_eq!(schema["properties"]["options"]["minItems"], 2);
    assert_eq!(schema["properties"]["options"]["maxItems"], 3);
    assert!(schema["properties"].get("allows_free_form").is_none());
}
