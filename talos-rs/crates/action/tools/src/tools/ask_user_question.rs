use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use talos_error::{TalosError, TalosResult};
use talos_types::{QuestionAnswer, QuestionOption, QuestionParams, UserActionResponse};

use crate::spec::{ToolBehaviorFlags, ToolResultPolicy, ToolVisibility};
use crate::{decode_tool_input, Tool, ToolContext, ToolDispatchOutcome, ToolOutputPayload};

pub struct AskUserQuestionTool;

#[derive(Debug, Deserialize)]
struct AskUserQuestionRequest {
    header: String,
    question: String,
    options: Vec<AskUserQuestionOption>,
}

#[derive(Debug, Deserialize)]
struct AskUserQuestionOption {
    id: String,
    label: String,
    description: String,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
struct QuestionToolResult {
    question: String,
    answer: QuestionToolResultAnswer,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
enum QuestionToolResultAnswer {
    SelectedOption { option_id: String, label: String },
    FreeForm { text: String },
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
                "header": "Output Preference",
                "question": "Choose response style",
                "options": [
                    {
                        "id": "concise",
                        "label": "Concise",
                        "description": "Keep the answer brief"
                    },
                    {
                        "id": "detailed",
                        "label": "Detailed",
                        "description": "Include supporting context"
                    }
                ]
            }],
            "properties": {
                "header": { "type": "string", "minLength": 1 },
                "question": { "type": "string", "minLength": 1 },
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
        let params = question_params(request);
        validate_question(&params)?;
        let call_id = context.current_call_id().ok_or_else(|| {
            TalosError::ToolError("AskUserQuestion is missing its tool call id".to_string())
        })?;
        let response = context.get_user_action_response(call_id).ok_or_else(|| {
            TalosError::ToolError("AskUserQuestion is missing its accepted answer".to_string())
        })?;
        let UserActionResponse::Question(answer) = response else {
            return Err(TalosError::ValidationError(
                "AskUserQuestion received a mismatched user action response".to_string(),
            ));
        };
        let result = question_tool_result(&params, answer)?;
        ToolOutputPayload::success_json(&result, "serialize AskUserQuestion result")
    }

    async fn execute_outcome(
        &self,
        input: Value,
        context: ToolContext,
    ) -> TalosResult<ToolDispatchOutcome> {
        let request: AskUserQuestionRequest = decode_tool_input(input.clone(), self.name())?;
        let params = question_params(request);
        validate_question(&params)?;
        let call_id = context.current_call_id().ok_or_else(|| {
            TalosError::ToolError("AskUserQuestion is missing its tool call id".to_string())
        })?;
        if context.get_user_action_response(call_id).is_none() {
            return Ok(ToolDispatchOutcome::pending_question(self.name(), params));
        }
        self.execute_payload(input, context)
            .await
            .map(ToolDispatchOutcome::executed)
    }
}

fn question_params(request: AskUserQuestionRequest) -> QuestionParams {
    QuestionParams {
        header: request.header,
        question: request.question,
        options: request
            .options
            .into_iter()
            .map(|option| QuestionOption {
                id: option.id,
                label: option.label,
                description: option.description,
            })
            .collect(),
    }
}

fn validate_question(params: &QuestionParams) -> TalosResult<()> {
    if params.header.trim().is_empty() || params.question.trim().is_empty() {
        return Err(TalosError::ValidationError(
            "question header and text cannot be empty".to_string(),
        ));
    }
    if !(2..=3).contains(&params.options.len()) {
        return Err(TalosError::ValidationError(
            "question options must contain 2 or 3 entries".to_string(),
        ));
    }
    let mut ids = std::collections::BTreeSet::new();
    for option in &params.options {
        if option.id.trim().is_empty()
            || option.label.trim().is_empty()
            || option.description.trim().is_empty()
        {
            return Err(TalosError::ValidationError(
                "question option id, label, and description cannot be empty".to_string(),
            ));
        }
        if !ids.insert(option.id.as_str()) {
            return Err(TalosError::ValidationError(format!(
                "duplicate question option id `{}`",
                option.id
            )));
        }
    }
    Ok(())
}

fn question_tool_result(
    params: &QuestionParams,
    answer: QuestionAnswer,
) -> TalosResult<QuestionToolResult> {
    let answer = match answer {
        QuestionAnswer::SelectedOption { option_id } => {
            let option = params
                .options
                .iter()
                .find(|option| option.id == option_id)
                .ok_or_else(|| {
                    TalosError::ValidationError(format!(
                        "question option `{option_id}` does not exist"
                    ))
                })?;
            QuestionToolResultAnswer::SelectedOption {
                option_id,
                label: option.label.clone(),
            }
        }
        QuestionAnswer::FreeForm { text } => {
            let text = text.trim().to_string();
            if text.is_empty() {
                return Err(TalosError::ValidationError(
                    "question free-form answer cannot be empty".to_string(),
                ));
            }
            if text.len() > 4096 {
                return Err(TalosError::ValidationError(
                    "question free-form answer exceeds 4096 bytes".to_string(),
                ));
            }
            QuestionToolResultAnswer::FreeForm { text }
        }
    };
    Ok(QuestionToolResult {
        question: params.question.clone(),
        answer,
    })
}

#[cfg(test)]
#[path = "ask_user_question_tests.rs"]
mod tests;
