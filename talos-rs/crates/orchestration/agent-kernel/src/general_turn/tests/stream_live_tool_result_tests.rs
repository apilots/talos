use super::*;
use serde_json::Value;
use talos_tools::{Tool, ToolContext, ToolOutputPayload};
use tokio::time::{timeout, Duration};

struct HoldingToolStreamProvider;
struct DelayedTextTool;

#[async_trait]
impl Tool for DelayedTextTool {
    fn name(&self) -> &str {
        "DelayedText"
    }

    fn description(&self) -> &str {
        "Return text after a short delay"
    }

    fn parameters_schema(&self) -> Option<Value> {
        Some(json!({ "type": "object", "additionalProperties": false }))
    }

    fn behavior_flags(&self) -> talos_tools::spec::ToolBehaviorFlags {
        talos_tools::spec::ToolBehaviorFlags {
            read_only: true,
            destructive: false,
            concurrency_safe: true,
            requires_user_interaction: false,
        }
    }

    async fn execute_payload(
        &self,
        _input: Value,
        _context: ToolContext,
    ) -> TalosResult<ToolOutputPayload> {
        tokio::time::sleep(Duration::from_millis(25)).await;
        Ok(ToolOutputPayload::success_text("delayed output"))
    }
}

#[async_trait]
impl LlmProvider for HoldingToolStreamProvider {
    fn name(&self) -> &str {
        "holding-tool-stream"
    }

    async fn chat(&self, _request: ChatRequest) -> TalosResult<ChatResponse> {
        panic!("chat should not be called in holding stream test");
    }

    async fn stream(&self, _request: ChatRequest) -> TalosResult<ChatStream> {
        let (_tx, rx) = mpsc::channel::<TalosResult<ChatChunk>>(1);
        Ok(ChatStream::new(rx))
    }

    async fn stream_model_blocks(&self, _request: ChatRequest) -> TalosResult<ModelBlockStream> {
        let (tx, rx) = mpsc::channel::<TalosResult<ModelBlockStreamEvent>>(8);
        tokio::spawn(async move {
            tx.send(Ok(ModelBlockStreamEvent::BlockStarted(
                ModelBlockStarted::tool_call(
                    0,
                    Some("call_delayed".to_string()),
                    Some("DelayedText".to_string()),
                ),
            )))
            .await
            .expect("tool start should send");
            tx.send(Ok(ModelBlockStreamEvent::BlockDelta {
                model_item_id: "tool_call_0".to_string(),
                delta: ModelBlockDelta::ToolArguments {
                    delta: json!({}).to_string(),
                },
            }))
            .await
            .expect("tool args should send");
            tx.send(Ok(ModelBlockStreamEvent::BlockCompleted(
                ModelBlockCompleted {
                    model_item_id: "tool_call_0".to_string(),
                },
            )))
            .await
            .expect("tool completed should send");
            std::future::pending::<()>().await;
            drop(tx);
        });
        Ok(ModelBlockStream::new(rx))
    }

    fn default_model(&self) -> String {
        "holding-tool-stream".to_string()
    }
}

#[tokio::test]
async fn stream_round_emits_tool_result_when_model_stream_stays_open() {
    let provider = Arc::new(HoldingToolStreamProvider);
    let request = ChatRequest::new_user("Run the delayed tool", "holding-tool-stream");
    let registry = Arc::new(ToolRegistry::new());
    registry.register(Arc::new(DelayedTextTool));
    let execution_context = ToolExecutionContext {
        session_id: "agent-a1".to_string(),
        turn_id: "turn-1".to_string(),
        agent_id: "agent-a1".to_string(),
        user_id: "talos".to_string(),
        project_root: None,
        kernel_config: Arc::new(AgentKernelConfig::default()),
        dynamic_tool_results: std::collections::HashMap::new(),
        user_action_responses: std::collections::HashMap::new(),
        sandbox_approval_fingerprints: std::collections::HashMap::new(),
        command_risk_assessor: None,
        run_tasks: crate::test_run_tasks(),
    };
    let (tx, mut rx) = mpsc::channel(16);

    let stream_round_future = stream_round(
        provider,
        request,
        0,
        KernelModelCallMode::WorkingLoop,
        None,
        Some(&tx),
        Some(StreamingToolExecutionConfig {
            execution_mode: ToolExecutionMode::Parallel,
            max_parallelism: 3,
            execution_context,
            tool_registry: registry,
        }),
    );
    tokio::pin!(stream_round_future);

    let saw_tool_result = timeout(Duration::from_millis(500), async {
        loop {
            tokio::select! {
                event = rx.recv() => {
                    let event = event.expect("stream event should arrive");
                    if let KernelStreamEventData::ToolResult(data) = event.data {
                        break data.call_id == "call_delayed";
                    }
                }
                result = &mut stream_round_future => {
                    panic!("stream round completed before streaming the tool result: {result:?}");
                }
            }
        }
    })
    .await
    .expect("tool result should stream while the model stream is still open");

    assert!(saw_tool_result);
}
