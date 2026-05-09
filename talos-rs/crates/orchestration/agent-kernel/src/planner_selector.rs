use std::sync::Arc;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use talos_llm_provider::{ChatMessage, ChatRequest, LlmProvider, Role};

use crate::{PlannerKind, SessionEvent, SessionEventPayload};

const CLASSIFIER_SYSTEM_PROMPT: &str = r#"You are a task routing assistant. Your sole job is to classify the user's request
into one of two execution modes and return a JSON object.

## Execution Modes

### builtin
Use when the task is conversational, analytical, or involves a bounded set of tool calls
where the outcome and required steps are clear upfront.

Includes:
- Pure Q&A, explanation, translation, summarization
- Reading a file and answering a question about it
- Running a single command and reporting the result
- Searching for information and returning it
- Analyzing provided content (code, logs, data)
- Explaining a specific module, file, function, log, or error trace without modifying anything
- Reviewing a codebase or subsystem and producing findings, summaries, or plans without making changes

### react
Use when the task requires multi-step planning where earlier results determine later steps,
or when the goal has inherent uncertainty about the execution path.

Includes:
- Implementing a new feature or module
- Refactoring or restructuring existing code
- Debugging and fixing an error (read → analyze → fix → verify cycle)
- Setting up pipelines, infrastructure, or multi-file changes
- Investigating a codebase or subsystem when the task explicitly includes follow-up execution such as modifying, fixing, refactoring, implementing, or verifying
- Any task where "what to do next" depends on what was just done

## Decision Rules

1. If no tools are available: always return "builtin".
2. "Read file X and explain Y" → builtin (single read + analysis, clear outcome).
3. "Read file X and implement Y following the same pattern" → react (read informs multi-step impl).
4. "Fix bug Z" without further context → react (requires read → diagnose → patch → verify).
5. "Analyze / review / explain this codebase / architecture / subsystem" → builtin unless the request explicitly asks to change, fix, refactor, implement, migrate, verify, or apply changes.
6. Repository-level or subsystem-level analysis without execution is still builtin if the expected output is findings, summary, assessment, plan, or review comments.
7. When uncertain between builtin and react: prefer "builtin" for analysis-only tasks; prefer "react" only when execution is clearly requested.

## Input

- `input`: the user's current message
- `recent_history`: last 1-3 conversation turns (user/assistant pairs, truncated to 200 chars each)
- `tools`: comma-separated list of available tool names, or "none"

## Output

Respond ONLY with a JSON object. No explanation, no markdown fences, no extra text.

{"mode": "builtin" | "react"}"#;

/// Common status for any LLM call result.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LlmCallStatus {
    Ok,
    Err { kind: LlmCallError, message: String },
}

impl LlmCallStatus {
    pub fn is_ok(&self) -> bool {
        matches!(self, Self::Ok)
    }
}

/// Failure categories for LLM calls.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LlmCallError {
    ProviderError,
    EmptyResponse,
    InvalidResponse,
    Timeout,
}

/// Input to the planner selector.
pub struct PlannerSelectionRequest {
    pub input: String,
    pub recent_events: Vec<SessionEvent>,
    pub tool_names: Vec<String>,
    pub model: String,
}

/// Output from the planner selector. Also used as the Planning event payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannerSelectionResponse {
    /// Selected planner. Holds the fallback value if selection failed.
    pub planner_kind: PlannerKind,
    pub latency_ms: u64,
    pub status: LlmCallStatus,
}

/// Selects the appropriate planner for each turn via a lightweight LLM call.
pub struct PlannerSelector {
    llm_provider: Arc<dyn LlmProvider>,
}

impl PlannerSelector {
    pub fn new(llm_provider: Arc<dyn LlmProvider>) -> Self {
        Self { llm_provider }
    }

    /// Select a planner for the given request.
    /// On failure, `planner_kind` holds `fallback` and `status` records the error.
    pub async fn select(
        &self,
        request: PlannerSelectionRequest,
        fallback: PlannerKind,
    ) -> PlannerSelectionResponse {
        let started = Instant::now();
        let result = self.call_llm(&request).await;
        let latency_ms = started.elapsed().as_millis() as u64;
        match result {
            Ok(kind) => PlannerSelectionResponse {
                planner_kind: kind,
                latency_ms,
                status: LlmCallStatus::Ok,
            },
            Err((error_kind, message)) => PlannerSelectionResponse {
                planner_kind: fallback,
                latency_ms,
                status: LlmCallStatus::Err {
                    kind: error_kind,
                    message,
                },
            },
        }
    }

    async fn call_llm(
        &self,
        request: &PlannerSelectionRequest,
    ) -> Result<PlannerKind, (LlmCallError, String)> {
        let user_content = build_user_content(request);

        let llm_request = ChatRequest {
            messages: vec![
                ChatMessage::system(CLASSIFIER_SYSTEM_PROMPT),
                ChatMessage::user(user_content),
            ],
            model: request.model.clone(),
            tools: None,
            temperature: Some(0.0),
            max_tokens: Some(32),
            stop: None,
            top_p: None,
            tool_choice: None,
            parallel_tool_calls: None,
            native_thinking: None,
        };

        let response = self
            .llm_provider
            .chat(llm_request)
            .await
            .map_err(|e| (LlmCallError::ProviderError, e.to_string()))?;

        let text = response.content.trim().to_string();
        if text.is_empty() {
            return Err((LlmCallError::EmptyResponse, "empty response".to_string()));
        }

        let v: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| (LlmCallError::InvalidResponse, e.to_string()))?;

        match v["mode"].as_str() {
            Some("builtin") => Ok(PlannerKind::BuiltIn),
            Some("react") => Ok(PlannerKind::PlanReAct),
            other => Err((
                LlmCallError::InvalidResponse,
                format!("unknown mode: {:?}", other),
            )),
        }
    }
}

fn build_user_content(request: &PlannerSelectionRequest) -> String {
    let tools_str = if request.tool_names.is_empty() {
        "none".to_string()
    } else {
        request.tool_names.join(", ")
    };
    let history = request
        .recent_events
        .iter()
        .filter_map(|event| match &event.payload {
            SessionEventPayload::Message { message } => {
                let role = match message.role {
                    Role::User => "user",
                    Role::Assistant => "assistant",
                    _ => return None,
                };
                Some(format!(
                    "{}: {}",
                    role,
                    truncate_for_selector(&message.content, 200)
                ))
            }
            _ => None,
        })
        .collect::<Vec<_>>();

    format!(
        "tools: {}\n\nrecent_history:\n{}\n\ninput: {}",
        tools_str,
        history.join("\n"),
        request.input
    )
}

fn truncate_for_selector(value: &str, max_chars: usize) -> String {
    let trimmed = value.trim();
    let truncated = trimmed.chars().take(max_chars).collect::<String>();
    if trimmed.chars().count() > max_chars {
        format!("{}...", truncated)
    } else {
        truncated
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EventActions, EventAuthor, ExecutionBranch, SessionEventKind};
    use talos_error::TalosResult;
    use talos_llm_provider::{ChatResponse, ChatStream, Usage};

    struct MockLlmProvider {
        mode: &'static str,
    }

    #[async_trait::async_trait]
    impl LlmProvider for MockLlmProvider {
        fn name(&self) -> &str {
            "mock"
        }
        fn default_model(&self) -> String {
            "mock".to_string()
        }
        async fn chat(&self, _request: ChatRequest) -> TalosResult<ChatResponse> {
            Ok(ChatResponse {
                content: format!(r#"{{"mode":"{}"}}"#, self.mode),
                reasoning: Vec::new(),
                model: "mock".to_string(),
                usage: Usage::default(),
                finish_reason: Some("stop".to_string()),
                tool_calls: None,
            })
        }
        async fn stream(&self, _request: ChatRequest) -> TalosResult<ChatStream> {
            unimplemented!()
        }
    }

    fn make_request(input: &str, tools: &[&str]) -> PlannerSelectionRequest {
        PlannerSelectionRequest {
            input: input.to_string(),
            recent_events: Vec::new(),
            tool_names: tools.iter().map(|s| s.to_string()).collect(),
            model: "mock".to_string(),
        }
    }

    #[tokio::test]
    async fn test_selector_returns_builtin() {
        let selector = PlannerSelector::new(Arc::new(MockLlmProvider { mode: "builtin" }));
        let resp = selector
            .select(make_request("explain X", &[]), PlannerKind::BuiltIn)
            .await;
        assert_eq!(resp.planner_kind, PlannerKind::BuiltIn);
        assert!(resp.status.is_ok());
    }

    #[tokio::test]
    async fn test_selector_returns_react() {
        let selector = PlannerSelector::new(Arc::new(MockLlmProvider { mode: "react" }));
        let resp = selector
            .select(
                make_request("implement feature X", &["Read", "Bash"]),
                PlannerKind::BuiltIn,
            )
            .await;
        assert_eq!(resp.planner_kind, PlannerKind::PlanReAct);
        assert!(resp.status.is_ok());
    }

    struct FailingProvider;

    #[async_trait::async_trait]
    impl LlmProvider for FailingProvider {
        fn name(&self) -> &str {
            "failing"
        }
        fn default_model(&self) -> String {
            "failing".to_string()
        }
        async fn chat(&self, _: ChatRequest) -> TalosResult<ChatResponse> {
            Err(talos_error::TalosError::InternalError(
                "provider down".to_string(),
            ))
        }
        async fn stream(&self, _: ChatRequest) -> TalosResult<ChatStream> {
            unimplemented!()
        }
    }

    #[tokio::test]
    async fn test_selector_falls_back_on_provider_error() {
        let selector = PlannerSelector::new(Arc::new(FailingProvider));
        let resp = selector
            .select(
                make_request("do something", &["tool"]),
                PlannerKind::BuiltIn,
            )
            .await;
        assert_eq!(resp.planner_kind, PlannerKind::BuiltIn);
        assert!(!resp.status.is_ok());
    }

    #[test]
    fn test_build_user_content_includes_recent_history() {
        let request = PlannerSelectionRequest {
            input: "implement follow-up".to_string(),
            recent_events: vec![
                SessionEvent {
                    id: "1".to_string(),
                    session_id: "s1".to_string(),
                    invocation_id: "i1".to_string(),
                    branch: ExecutionBranch::ReactiveLoop,
                    author: EventAuthor::User,
                    kind: SessionEventKind::UserMessage,
                    payload: SessionEventPayload::Message {
                        message: ChatMessage::user("please inspect foo.rs first"),
                    },
                    actions: EventActions::default(),
                    partial: false,
                    timestamp: chrono::Utc::now(),
                },
                SessionEvent {
                    id: "2".to_string(),
                    session_id: "s1".to_string(),
                    invocation_id: "i1".to_string(),
                    branch: ExecutionBranch::ReactiveLoop,
                    author: EventAuthor::Agent,
                    kind: SessionEventKind::AgentMessage,
                    payload: SessionEventPayload::Message {
                        message: ChatMessage::assistant("I found the existing endpoint pattern"),
                    },
                    actions: EventActions::default(),
                    partial: false,
                    timestamp: chrono::Utc::now(),
                },
            ],
            tool_names: vec!["Read".to_string()],
            model: "mock".to_string(),
        };

        let content = build_user_content(&request);
        assert!(content.contains("recent_history:"));
        assert!(content.contains("user: please inspect foo.rs first"));
        assert!(content.contains("assistant: I found the existing endpoint pattern"));
    }
}
