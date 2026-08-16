use std::collections::VecDeque;
use std::sync::Mutex;

use async_trait::async_trait;
use talos_error::{TalosError, TalosResult};
use talos_llm_provider::{
    ChatChunk, ChatRequest, ChatResponse, ChatStream, FunctionCall, LlmProvider, ReasoningOutput,
    ToolChoice, Usage,
};
use talos_tools_sandbox::{
    CommandRisk, CommandRiskCategory, CommandRiskVerdict, FileSystemPermissionLevel,
    NetworkSandboxPolicy,
};
use tokio::sync::mpsc;

use super::*;

struct TestProvider {
    responses: Mutex<VecDeque<TalosResult<ChatResponse>>>,
    requests: Mutex<Vec<ChatRequest>>,
    delay: Duration,
}

impl TestProvider {
    fn with_responses(responses: Vec<TalosResult<ChatResponse>>) -> Self {
        Self {
            responses: Mutex::new(responses.into()),
            requests: Mutex::new(Vec::new()),
            delay: Duration::ZERO,
        }
    }

    fn delayed(delay: Duration) -> Self {
        Self {
            responses: Mutex::new(VecDeque::from([Ok(tool_response(serde_json::json!({
                "verdict": "safe",
                "categories": [],
                "summary": "safe"
            })))])),
            requests: Mutex::new(Vec::new()),
            delay,
        }
    }
}

#[async_trait]
impl LlmProvider for TestProvider {
    fn name(&self) -> &str {
        "command-risk-test"
    }

    async fn chat(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        self.requests
            .lock()
            .expect("requests should lock")
            .push(request);
        if !self.delay.is_zero() {
            tokio::time::sleep(self.delay).await;
        }
        self.responses
            .lock()
            .expect("responses should lock")
            .pop_front()
            .expect("response should exist")
    }

    async fn stream(&self, _request: ChatRequest) -> TalosResult<ChatStream> {
        let (_tx, rx) = mpsc::channel::<TalosResult<ChatChunk>>(1);
        Ok(ChatStream::new(rx))
    }

    fn default_model(&self) -> String {
        "risk-model".to_string()
    }
}

fn tool_response(arguments: serde_json::Value) -> ChatResponse {
    ChatResponse {
        content: String::new(),
        reasoning: ReasoningOutput::default(),
        model: "risk-model".to_string(),
        usage: Usage::default(),
        finish_reason: Some("tool_calls".to_string()),
        tool_calls: Some(vec![FunctionCall::new(
            "risk-call",
            COMMAND_RISK_TOOL_NAME,
            arguments,
        )]),
    }
}

fn text_response(content: &str) -> ChatResponse {
    ChatResponse {
        content: content.to_string(),
        reasoning: ReasoningOutput::default(),
        model: "risk-model".to_string(),
        usage: Usage::default(),
        finish_reason: Some("stop".to_string()),
        tool_calls: None,
    }
}

fn input(command: &str) -> CommandRiskAssessmentInput {
    CommandRiskAssessmentInput::new(
        command,
        std::path::Path::new("/workspace"),
        classify_bash_command(command),
        EffectivePermissionSummary {
            file_system: FileSystemPermissionLevel::WorkspaceWrite,
            network: NetworkSandboxPolicy::Enabled,
            sandbox_enabled: true,
        },
    )
}

#[test]
fn deterministic_classifier_separates_safe_dangerous_and_ambiguous_commands() {
    assert_eq!(classify_bash_command("git status").risk, CommandRisk::Safe);
    assert_eq!(
        classify_bash_command("rm -rf build").risk,
        CommandRisk::Dangerous
    );
    assert_eq!(
        classify_bash_command("cargo test -p talos-tools").risk,
        CommandRisk::Ambiguous
    );
    assert_eq!(
        classify_bash_command("rg TODO | head").risk,
        CommandRisk::Ambiguous
    );
}

#[tokio::test]
async fn structured_assessor_accepts_only_typed_verdicts() {
    let provider = Arc::new(TestProvider::with_responses(vec![
        Ok(tool_response(serde_json::json!({
            "verdict": "safe", "categories": [], "summary": "No risk found"
        }))),
        Ok(tool_response(serde_json::json!({
            "verdict": "risky",
            "categories": ["remote_code_execution"],
            "summary": "Executes remote content"
        }))),
        Ok(tool_response(serde_json::json!({
            "verdict": "unknown", "categories": ["unknown"], "summary": "Effects are unclear"
        }))),
    ]));
    let assessor = LlmCommandRiskAssessor::new(provider, "risk-model");

    let safe = assessor.assess(input("cargo test")).await.expect("safe");
    let risky = assessor
        .assess(input("custom-script"))
        .await
        .expect("risky");
    let unknown = assessor
        .assess(input("other-script"))
        .await
        .expect("unknown");

    assert_eq!(safe.verdict, CommandRiskVerdict::Safe);
    assert_eq!(risky.verdict, CommandRiskVerdict::Risky);
    assert_eq!(
        risky.categories,
        vec![CommandRiskCategory::RemoteCodeExecution]
    );
    assert_eq!(unknown.verdict, CommandRiskVerdict::Unknown);
}

#[tokio::test]
async fn structured_assessor_rejects_invalid_schema_and_bounds() {
    let provider = Arc::new(TestProvider::with_responses(vec![
        Ok(tool_response(serde_json::json!({
            "verdict": "safe", "categories": [], "summary": "safe", "approval": true
        }))),
        Ok(tool_response(serde_json::json!({
            "verdict": "safe",
            "categories": [],
            "summary": "x".repeat(SUMMARY_MAX_CHARS + 1)
        }))),
    ]));
    let assessor = LlmCommandRiskAssessor::new(provider, "risk-model");

    assert!(matches!(
        assessor.assess(input("custom")).await,
        Err(CommandRiskAssessmentError::InvalidOutput { .. })
    ));
    assert!(matches!(
        assessor.assess(input("custom")).await,
        Err(CommandRiskAssessmentError::InvalidOutput { .. })
    ));
}

#[tokio::test]
async fn structured_assessor_rejects_text_and_wrong_tool_shapes() {
    let mut multiple = tool_response(serde_json::json!({
        "verdict": "safe", "categories": [], "summary": "safe"
    }));
    multiple
        .tool_calls
        .as_mut()
        .expect("tool calls should exist")
        .push(FunctionCall::new(
            "risk-call-2",
            COMMAND_RISK_TOOL_NAME,
            serde_json::json!({
                "verdict": "safe", "categories": [], "summary": "safe"
            }),
        ));
    let provider = Arc::new(TestProvider::with_responses(vec![
        Ok(text_response(
            r#"{"verdict":"safe","categories":[],"summary":"safe"}"#,
        )),
        Ok(multiple),
        Ok(ChatResponse {
            tool_calls: Some(vec![FunctionCall::new(
                "risk-call",
                "OtherTool",
                serde_json::json!({
                    "verdict": "safe", "categories": [], "summary": "safe"
                }),
            )]),
            ..text_response("")
        }),
    ]));
    let assessor = LlmCommandRiskAssessor::new(provider, "risk-model");

    for _ in 0..3 {
        assert!(matches!(
            assessor.assess(input("custom")).await,
            Err(CommandRiskAssessmentError::InvalidOutput { .. })
        ));
    }
}

#[tokio::test]
async fn structured_assessor_reports_unavailable_provider_and_timeout() {
    let unavailable = Arc::new(TestProvider::with_responses(vec![Err(
        TalosError::internal("provider unavailable"),
    )]));
    let assessor = LlmCommandRiskAssessor::new(unavailable, "risk-model");
    assert!(matches!(
        assessor.assess(input("custom")).await,
        Err(CommandRiskAssessmentError::Unavailable { .. })
    ));

    let delayed = Arc::new(TestProvider::delayed(Duration::from_millis(50)));
    let assessor =
        LlmCommandRiskAssessor::new(delayed, "risk-model").with_timeout(Duration::from_millis(1));
    assert_eq!(
        assessor.assess(input("custom")).await,
        Err(CommandRiskAssessmentError::Timeout)
    );
}

#[tokio::test]
async fn assessor_input_is_bounded_before_reaching_the_provider() {
    let provider = Arc::new(TestProvider::with_responses(vec![Ok(tool_response(
        serde_json::json!({
            "verdict": "safe", "categories": [], "summary": "safe"
        }),
    ))]));
    let assessor = LlmCommandRiskAssessor::new(provider.clone(), "risk-model");

    assessor
        .assess(input(&"x".repeat(COMMAND_MAX_CHARS + 100)))
        .await
        .expect("assessment should succeed");

    let requests = provider.requests.lock().expect("requests should lock");
    let payload = &requests[0].messages[1].content;
    let decoded: CommandRiskAssessmentInput =
        serde_json::from_str(payload).expect("input should be typed json");
    assert_eq!(decoded.command.chars().count(), COMMAND_MAX_CHARS);
    assert_eq!(requests[0].tool_choice, Some(ToolChoice::Auto));
    assert_eq!(requests[0].max_tokens, Some(512));
    assert_eq!(requests[0].parallel_tool_calls, Some(false));
    assert_eq!(requests[0].tools.as_ref().map(Vec::len), Some(1));
}
