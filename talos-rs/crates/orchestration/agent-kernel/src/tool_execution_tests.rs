use super::*;
use crate::{KernelStreamEventData, ToolProgressKind, ToolStreamKey};
use async_trait::async_trait;
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use talos_tools::spec::ToolBehaviorFlags;
use talos_tools::{register_core_local_tools, Tool, ToolContext, ToolOutputPayload, ToolRegistry};
use tokio::sync::mpsc;
use tokio::time::{sleep, Duration};

struct ObservedDelayTool {
    name: &'static str,
    concurrent: Arc<AtomicUsize>,
    max_concurrent: Arc<AtomicUsize>,
}

struct AgentIdentityTool {
    observed_agent_id: Arc<Mutex<Option<String>>>,
}

#[async_trait]
impl Tool for AgentIdentityTool {
    fn name(&self) -> &str {
        "Read"
    }

    fn description(&self) -> &str {
        "Observe the trusted Agent identity"
    }

    fn parameters_schema(&self) -> Option<Value> {
        Some(json!({ "type": "object", "additionalProperties": false }))
    }

    async fn execute_payload(
        &self,
        _input: Value,
        context: ToolContext,
    ) -> TalosResult<ToolOutputPayload> {
        self.observed_agent_id
            .lock()
            .unwrap()
            .replace(context.agent_id().unwrap_or_default().to_string());
        Ok(ToolOutputPayload::success_text("ok"))
    }
}

#[async_trait]
impl Tool for ObservedDelayTool {
    fn name(&self) -> &str {
        self.name
    }

    fn description(&self) -> &str {
        "Return the provided label after a configurable delay"
    }

    fn behavior_flags(&self) -> ToolBehaviorFlags {
        ToolBehaviorFlags {
            read_only: self.name != "Write",
            destructive: self.name == "Write",
            concurrency_safe: self.name != "Write",
            requires_user_interaction: false,
        }
    }

    fn parameters_schema(&self) -> Option<Value> {
        Some(json!({
            "type": "object",
            "required": ["label", "delay_ms"],
            "properties": {
                "label": { "type": "string" },
                "delay_ms": { "type": "integer" }
            }
        }))
    }

    async fn execute_payload(
        &self,
        input: Value,
        _context: ToolContext,
    ) -> TalosResult<ToolOutputPayload> {
        let label = input
            .get("label")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let delay_ms = input
            .get("delay_ms")
            .and_then(Value::as_u64)
            .unwrap_or_default();
        let active = self.concurrent.fetch_add(1, Ordering::SeqCst) + 1;
        let _ = self.max_concurrent.fetch_max(active, Ordering::SeqCst);
        sleep(Duration::from_millis(delay_ms)).await;
        self.concurrent.fetch_sub(1, Ordering::SeqCst);
        Ok(ToolOutputPayload::success_text(label))
    }
}

#[tokio::test]
async fn tool_execution_context_propagates_the_trusted_agent_id() {
    let registry = Arc::new(ToolRegistry::new());
    let observed_agent_id = Arc::new(Mutex::new(None));
    registry.register(Arc::new(AgentIdentityTool {
        observed_agent_id: observed_agent_id.clone(),
    }));
    let execution_context = ToolExecutionContext {
        session_id: "s1".to_string(),
        turn_id: "turn-1".to_string(),
        agent_id: "reviewer".to_string(),
        user_id: "u1".to_string(),
        project_root: None,
        kernel_config: Arc::new(AgentKernelConfig::default()),
        dynamic_tool_results: HashMap::new(),
    };

    SequentialToolExecutor
        .execute(
            &execution_context,
            registry,
            &[ToolExecutionInput {
                call_id: "call-1".to_string(),
                tool_name: "Read".to_string(),
                arguments: json!({}),
            }],
        )
        .await
        .unwrap();

    assert_eq!(
        observed_agent_id.lock().unwrap().as_deref(),
        Some("reviewer")
    );
}

#[tokio::test]
async fn test_parallel_tool_executor_preserves_input_order() {
    let registry = Arc::new(ToolRegistry::new());
    let concurrent = Arc::new(AtomicUsize::new(0));
    let max_concurrent = Arc::new(AtomicUsize::new(0));
    registry.register(Arc::new(ObservedDelayTool {
        name: "Read",
        concurrent: concurrent.clone(),
        max_concurrent: max_concurrent.clone(),
    }));
    let executor = ParallelToolExecutor;
    let execution_context = ToolExecutionContext {
        session_id: "s1".to_string(),
        turn_id: "turn-1".to_string(),
        agent_id: "agent-1".to_string(),
        user_id: "u1".to_string(),
        project_root: None,
        kernel_config: Arc::new(AgentKernelConfig::default()),
        dynamic_tool_results: HashMap::new(),
    };
    let calls = vec![
        ToolExecutionInput {
            call_id: "call_1".to_string(),
            tool_name: "Read".to_string(),
            arguments: json!({
                "label": "first",
                "delay_ms": 50
            }),
        },
        ToolExecutionInput {
            call_id: "call_2".to_string(),
            tool_name: "Read".to_string(),
            arguments: json!({
                "label": "second",
                "delay_ms": 1
            }),
        },
    ];

    let outputs = executor
        .execute(&execution_context, registry, &calls)
        .await
        .expect("parallel execution should succeed");

    assert_eq!(outputs.len(), 2);
    assert_eq!(outputs[0].call_id, "call_1");
    assert_eq!(outputs[1].call_id, "call_2");
    assert_eq!(outputs[0].tool_name, "Read");
    assert_eq!(outputs[1].tool_name, "Read");
    assert!(max_concurrent.load(Ordering::SeqCst) >= 2);
}

#[tokio::test]
async fn test_parallel_tool_executor_falls_back_for_mutating_batch() {
    let registry = Arc::new(ToolRegistry::new());
    let concurrent = Arc::new(AtomicUsize::new(0));
    let max_concurrent = Arc::new(AtomicUsize::new(0));
    registry.register(Arc::new(ObservedDelayTool {
        name: "Read",
        concurrent: concurrent.clone(),
        max_concurrent: max_concurrent.clone(),
    }));
    registry.register(Arc::new(ObservedDelayTool {
        name: "Write",
        concurrent,
        max_concurrent: max_concurrent.clone(),
    }));

    let executor = ParallelToolExecutor;
    let execution_context = ToolExecutionContext {
        session_id: "s1".to_string(),
        turn_id: "turn-1".to_string(),
        agent_id: "agent-1".to_string(),
        user_id: "u1".to_string(),
        project_root: None,
        kernel_config: Arc::new(AgentKernelConfig::default()),
        dynamic_tool_results: HashMap::new(),
    };
    let calls = vec![
        ToolExecutionInput {
            call_id: "call_1".to_string(),
            tool_name: "Read".to_string(),
            arguments: json!({
                "label": "read",
                "delay_ms": 50
            }),
        },
        ToolExecutionInput {
            call_id: "call_2".to_string(),
            tool_name: "Write".to_string(),
            arguments: json!({
                "label": "write",
                "delay_ms": 50
            }),
        },
    ];

    let outputs = executor
        .execute(&execution_context, registry, &calls)
        .await
        .expect("mixed batch execution should succeed");

    assert_eq!(outputs.len(), 2);
    assert_eq!(outputs[0].call_id, "call_1");
    assert_eq!(outputs[1].call_id, "call_2");
    assert_eq!(max_concurrent.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn test_streaming_scheduler_forwards_tool_progress() {
    let registry = Arc::new(ToolRegistry::new());
    register_core_local_tools(&registry);
    let (stream_tx, mut stream_rx) = mpsc::channel(16);
    let execution_context = ToolExecutionContext {
        session_id: "s1".to_string(),
        turn_id: "turn-1".to_string(),
        agent_id: "agent-1".to_string(),
        user_id: "u1".to_string(),
        project_root: None,
        kernel_config: Arc::new(AgentKernelConfig::default()),
        dynamic_tool_results: HashMap::new(),
    };
    let mut scheduler = StreamingToolScheduler::new(
        ToolExecutionMode::Sequential,
        execution_context,
        registry,
        Some(stream_tx),
    );

    scheduler
        .push_call(
            ToolStreamKey { round: 0, index: 0 },
            ToolExecutionInput {
                call_id: "call_progress".to_string(),
                tool_name: "Bash".to_string(),
                arguments: json!({
                    "command": "printf 'hello from progress\\n'",
                }),
            },
        )
        .await
        .expect("tool should schedule");
    let outputs = scheduler.finish().await.expect("tool should finish");

    assert_eq!(outputs.len(), 1);
    assert!(!outputs[0].output.is_error);
    let mut saw_tool_progress = false;
    while let Ok(event) = stream_rx.try_recv() {
        if let KernelStreamEventData::ToolProgressDelta(data) = event.data {
            if data.call_id == "call_progress"
                && data.progress_kind == ToolProgressKind::Stdout
                && data.message.contains("hello from progress")
            {
                saw_tool_progress = true;
            }
        }
    }
    assert!(saw_tool_progress);
}
