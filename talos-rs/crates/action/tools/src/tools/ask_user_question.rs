use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;
use talos_error::TalosResult;

use crate::runtimes::task_state_runtime::TaskStateRuntime;
use crate::runtimes::task_state_types::StructuredQuestionOption;
use crate::spec::{ToolBehaviorFlags, ToolResultPolicy, ToolVisibility};
use crate::{decode_tool_input, Tool, ToolContext, ToolOutputPayload};

pub struct AskUserQuestionTool;

#[derive(Debug, Deserialize)]
struct AskUserQuestionRequest {
    header: String,
    question: String,
    options: Vec<AskUserQuestionOption>,
    #[serde(default)]
    allows_free_form: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct AskUserQuestionOption {
    id: String,
    label: String,
    description: String,
}

#[async_trait]
impl Tool for AskUserQuestionTool {
    fn name(&self) -> &str {
        "AskUserQuestion"
    }

    fn internal_name(&self) -> &str {
        "ask_user_question"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["ask_user_question"]
    }

    fn description(&self) -> &str {
        "Ask the user a multiple-choice question to clarify ambiguity, gather preferences, or make an execution decision. Provide 2-3 options, and every option must include a non-empty id, label, and description."
    }

    fn visibility(&self) -> ToolVisibility {
        ToolVisibility::DefaultVisible
    }

    fn behavior_flags(&self) -> ToolBehaviorFlags {
        ToolBehaviorFlags {
            read_only: false,
            destructive: false,
            concurrency_safe: false,
            requires_user_interaction: true,
        }
    }

    fn result_policy(&self) -> ToolResultPolicy {
        ToolResultPolicy {
            max_inline_bytes: 16 * 1024,
            persist_large_output: false,
            preview_lines: 30,
        }
    }

    fn parameters_schema(&self) -> Option<Value> {
        Some(serde_json::json!({
            "type": "object",
            "required": ["header", "question", "options"],
            "examples": [{
                "header": "Execution Mode",
                "question": "Choose mode",
                "options": [
                    {
                        "id": "plan",
                        "label": "Plan",
                        "description": "Explore the task before implementation"
                    },
                    {
                        "id": "direct",
                        "label": "Direct",
                        "description": "Execute immediately with minimal planning"
                    }
                ]
            }],
            "properties": {
                "header": { "type": "string", "minLength": 1 },
                "question": { "type": "string", "minLength": 1 },
                "allows_free_form": { "type": "boolean" },
                "options": {
                    "type": "array",
                    "minItems": 2,
                    "maxItems": 3,
                    "items": {
                        "type": "object",
                        "required": ["id", "label", "description"],
                        "properties": {
                            "id": { "type": "string", "minLength": 1 },
                            "label": { "type": "string", "minLength": 1 },
                            "description": { "type": "string", "minLength": 1 }
                        }
                    }
                }
            }
        }))
    }

    async fn execute_payload(
        &self,
        input: Value,
        context: ToolContext,
    ) -> TalosResult<ToolOutputPayload> {
        let request: AskUserQuestionRequest = decode_tool_input(input, self.name())?;
        let options = request
            .options
            .into_iter()
            .map(|option| StructuredQuestionOption {
                id: option.id,
                label: option.label,
                description: option.description,
            })
            .collect::<Vec<_>>();
        let record = TaskStateRuntime::global().ask_question(
            context.session_id(),
            request.header,
            request.question,
            options,
            request.allows_free_form.unwrap_or(true),
        )?;

        let value = serde_json::json!({
            "awaiting_user_response": true,
            "question": record,
        });
        ToolOutputPayload::success_json(&value, "serialize ask_user_question")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtimes::task_state_runtime::TaskStateRuntime;
    use crate::runtimes::task_state_types::StructuredQuestionStatus;
    use uuid::Uuid;

    #[tokio::test]
    async fn ask_user_question_records_pending_question() {
        let tool = AskUserQuestionTool;
        let session_id = format!("ask-user-{}", Uuid::new_v4());

        let payload = tool
            .execute_payload(
                serde_json::json!({
                    "header": "Execution Mode",
                    "question": "How should the agent proceed?",
                    "options": [
                        { "id": "plan", "label": "Plan", "description": "Think first" },
                        { "id": "direct", "label": "Direct", "description": "Act now" }
                    ]
                }),
                ToolContext::new(&session_id, "user-1"),
            )
            .await
            .expect("ask_user_question should succeed");

        let ToolOutputPayload::Function {
            structured_content, ..
        } = payload
        else {
            panic!("expected function payload");
        };
        assert_eq!(
            structured_content
                .as_ref()
                .and_then(|value| value.get("awaiting_user_response"))
                .and_then(Value::as_bool),
            Some(true)
        );

        let record = TaskStateRuntime::global()
            .latest_question(&session_id)
            .expect("question lookup should succeed")
            .expect("question should be stored");
        assert_eq!(record.status, StructuredQuestionStatus::Pending);
        assert_eq!(record.request.header, "Execution Mode");
    }
}
