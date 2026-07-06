use crate::execution::ReplayDynamicToolResult;
use crate::ToolExecutionMode;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use talos_config::AgentKernelConfig;
use talos_error::{TalosError, TalosResult, TalosToolErrorKind};
use talos_memory::MemoryStore;
use talos_tools::{
    ToolDispatchOutcome, ToolExecutorPort, ToolOutputPayload, ToolProgressEmitter,
    ToolProgressEvent,
};
use tokio::sync::mpsc;
use tokio::task::JoinSet;

#[derive(Debug, Clone)]
pub struct ToolExecutionContext {
    pub session_id: String,
    pub user_id: String,
    pub project_root: Option<PathBuf>,
    pub kernel_config: Arc<AgentKernelConfig>,
    pub memory_store: Arc<MemoryStore>,
    pub dynamic_tool_results: HashMap<String, ReplayDynamicToolResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolExecutionInput {
    pub call_id: String,
    pub tool_name: String,
    #[serde(default)]
    pub arguments: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolExecutionOutput {
    pub call_id: String,
    pub tool_name: String,
    pub outcome: ToolDispatchOutcome,
    #[serde(default)]
    pub is_error: bool,
}

#[async_trait]
pub trait ToolExecutor: Send + Sync {
    fn mode(&self) -> ToolExecutionMode;

    async fn execute(
        &self,
        _execution_context: &ToolExecutionContext,
        _registry: Arc<dyn ToolExecutorPort>,
        _calls: &[ToolExecutionInput],
    ) -> TalosResult<Vec<ToolExecutionOutput>> {
        Ok(Vec::new())
    }
}

pub struct SequentialToolExecutor;

fn build_tool_context(
    execution_context: &ToolExecutionContext,
    registry: &dyn ToolExecutorPort,
    call_id: &str,
    progress_emitter: Option<ToolProgressEmitter>,
) -> talos_tools::ToolContext {
    let ctx = registry.build_context(
        execution_context.session_id.clone(),
        execution_context.user_id.clone(),
        Some(execution_context.kernel_config.clone()),
    );
    let ctx = if let Some(project_root) = execution_context.project_root.clone() {
        ctx.with_project_root(project_root)
    } else {
        ctx
    };
    let ctx = ctx.with_memory_store(execution_context.memory_store.clone());
    if let Some(result) = execution_context.dynamic_tool_results.get(call_id) {
        ctx.set_dynamic_tool_result(
            call_id.to_string(),
            talos_tools::handlers::dynamic::StoredDynamicToolResult {
                output: result.output.clone(),
                success: result.success,
            },
        );
    }
    if let Some(emitter) = progress_emitter {
        ctx.with_progress_emitter(emitter)
    } else {
        ctx
    }
}

async fn execute_single_tool_call(
    registry: Arc<dyn ToolExecutorPort>,
    execution_context: &ToolExecutionContext,
    call: ToolExecutionInput,
    progress_emitter: Option<ToolProgressEmitter>,
) -> ToolExecutionOutput {
    let tool_context = build_tool_context(
        execution_context,
        registry.as_ref(),
        &call.call_id,
        progress_emitter,
    );
    let result = match call.build_invocation(registry.as_ref()) {
        Ok(invocation) => {
            registry
                .execute_invocation_with_context(invocation, tool_context)
                .await
        }
        Err(error) => Err(error),
    };
    match result {
        Ok(payload) => ToolExecutionOutput {
            call_id: call.call_id,
            tool_name: call.tool_name,
            is_error: payload
                .payload()
                .map(|item| item.is_error())
                .unwrap_or(false),
            outcome: payload,
        },
        Err(error) => ToolExecutionOutput {
            call_id: call.call_id,
            tool_name: call.tool_name,
            outcome: tool_error_outcome(error),
            is_error: true,
        },
    }
}

fn tool_error_outcome(error: TalosError) -> ToolDispatchOutcome {
    match error {
        TalosError::ToolFailure {
            kind,
            message,
            gate_source,
        } => structured_tool_failure(kind, message, gate_source),
        other => ToolDispatchOutcome::executed(ToolOutputPayload::Mcp {
            content: other.to_string(),
            is_error: true,
        }),
    }
}

fn structured_tool_failure(
    kind: TalosToolErrorKind,
    message: String,
    gate_source: Option<String>,
) -> ToolDispatchOutcome {
    let mut payload = json!({
        "error": message.clone(),
        "error_kind": kind,
    });
    if let Some(gate_source) = gate_source {
        if let Some(object) = payload.as_object_mut() {
            object.insert("gate_source".to_string(), Value::String(gate_source));
        }
    }
    ToolDispatchOutcome::executed(ToolOutputPayload::failure_structured(
        message,
        Some(payload),
    ))
}

#[async_trait]
impl ToolExecutor for SequentialToolExecutor {
    fn mode(&self) -> ToolExecutionMode {
        ToolExecutionMode::Sequential
    }

    async fn execute(
        &self,
        execution_context: &ToolExecutionContext,
        registry: Arc<dyn ToolExecutorPort>,
        calls: &[ToolExecutionInput],
    ) -> TalosResult<Vec<ToolExecutionOutput>> {
        let mut outputs = Vec::with_capacity(calls.len());
        for call in calls {
            outputs.push(
                execute_single_tool_call(registry.clone(), execution_context, call.clone(), None)
                    .await,
            );
        }
        Ok(outputs)
    }
}

pub struct ParallelToolExecutor;

impl ParallelToolExecutor {
    fn batch_allows_parallel(
        registry: &dyn ToolExecutorPort,
        calls: &[ToolExecutionInput],
    ) -> bool {
        !calls.is_empty()
            && calls.iter().all(|call| {
                registry.tool_supports_parallel(&call.tool_name)
                    && !registry.tool_is_mutating(&call.tool_name)
            })
    }
}

#[async_trait]
impl ToolExecutor for ParallelToolExecutor {
    fn mode(&self) -> ToolExecutionMode {
        ToolExecutionMode::Parallel
    }

    async fn execute(
        &self,
        execution_context: &ToolExecutionContext,
        registry: Arc<dyn ToolExecutorPort>,
        calls: &[ToolExecutionInput],
    ) -> TalosResult<Vec<ToolExecutionOutput>> {
        if !Self::batch_allows_parallel(registry.as_ref(), calls) {
            return SequentialToolExecutor
                .execute(execution_context, registry, calls)
                .await;
        }

        let mut join_set = JoinSet::new();
        for (index, call) in calls.iter().cloned().enumerate() {
            let registry = registry.clone();
            let tool_context =
                build_tool_context(execution_context, registry.as_ref(), &call.call_id, None);
            join_set.spawn(async move {
                let result = match call.build_invocation(registry.as_ref()) {
                    Ok(invocation) => {
                        registry
                            .execute_invocation_with_context(invocation, tool_context)
                            .await
                    }
                    Err(error) => Err(error),
                };
                let output = match result {
                    Ok(payload) => ToolExecutionOutput {
                        call_id: call.call_id,
                        tool_name: call.tool_name,
                        is_error: payload
                            .payload()
                            .map(|item| item.is_error())
                            .unwrap_or(false),
                        outcome: payload,
                    },
                    Err(error) => ToolExecutionOutput {
                        call_id: call.call_id,
                        tool_name: call.tool_name,
                        outcome: tool_error_outcome(error),
                        is_error: true,
                    },
                };
                (index, output)
            });
        }

        let mut outputs = vec![None; calls.len()];
        while let Some(result) = join_set.join_next().await {
            let (index, output) = result.map_err(|error| {
                TalosError::InternalError(format!("parallel tool execution task failed: {error}"))
            })?;
            outputs[index] = Some(output);
        }
        outputs
            .into_iter()
            .enumerate()
            .map(|(index, output)| {
                output.ok_or_else(|| {
                    TalosError::InternalError(format!(
                        "missing parallel tool output at index {index}"
                    ))
                })
            })
            .collect()
    }
}

pub struct ToolExecutorFactory;

impl ToolExecutorFactory {
    pub fn build(mode: ToolExecutionMode) -> Arc<dyn ToolExecutor> {
        match mode {
            ToolExecutionMode::Sequential => Arc::new(SequentialToolExecutor),
            ToolExecutionMode::Parallel => Arc::new(ParallelToolExecutor),
        }
    }
}

impl ToolExecutionInput {
    pub fn build_invocation(
        &self,
        registry: &dyn ToolExecutorPort,
    ) -> TalosResult<talos_tools::ToolInvocation> {
        registry.build_function_invocation(
            self.call_id.clone(),
            self.tool_name.clone(),
            self.arguments.clone(),
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StreamingToolSchedulingClass {
    ParallelSafe,
    SerialOnly,
}

#[derive(Debug, Clone)]
struct ScheduledStreamingToolCall {
    stream_key: crate::ToolStreamKey,
    input: ToolExecutionInput,
    scheduling_class: StreamingToolSchedulingClass,
}

#[derive(Debug, Clone)]
struct RunningStreamingToolCall {
    stream_key: crate::ToolStreamKey,
    scheduling_class: StreamingToolSchedulingClass,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamingToolExecutionOutput {
    pub stream_key: crate::ToolStreamKey,
    pub output: ToolExecutionOutput,
}

pub struct StreamingToolScheduler {
    execution_mode: ToolExecutionMode,
    execution_context: ToolExecutionContext,
    registry: Arc<dyn ToolExecutorPort>,
    stream_tx: Option<mpsc::Sender<crate::KernelStreamEvent>>,
    pending_calls: VecDeque<ScheduledStreamingToolCall>,
    running_tasks: JoinSet<(RunningStreamingToolCall, ToolExecutionOutput)>,
    parallel_running: usize,
    serial_running: bool,
}

impl StreamingToolScheduler {
    pub fn new(
        execution_mode: ToolExecutionMode,
        execution_context: ToolExecutionContext,
        registry: Arc<dyn ToolExecutorPort>,
        stream_tx: Option<mpsc::Sender<crate::KernelStreamEvent>>,
    ) -> Self {
        Self {
            execution_mode,
            execution_context,
            registry,
            stream_tx,
            pending_calls: VecDeque::new(),
            running_tasks: JoinSet::new(),
            parallel_running: 0,
            serial_running: false,
        }
    }

    pub async fn push_call(
        &mut self,
        stream_key: crate::ToolStreamKey,
        input: ToolExecutionInput,
    ) -> TalosResult<()> {
        let scheduled = ScheduledStreamingToolCall {
            stream_key: stream_key.clone(),
            scheduling_class: self.classify_call(&input.tool_name),
            input,
        };
        self.emit_queued_event(&scheduled).await;
        self.pending_calls.push_back(scheduled);
        self.schedule_ready_calls().await;
        Ok(())
    }

    pub async fn drain_completed(&mut self) -> TalosResult<Vec<StreamingToolExecutionOutput>> {
        let mut completed = Vec::new();
        while let Some(result) = self.running_tasks.try_join_next() {
            let (running_call, output) = result.map_err(|error| {
                TalosError::InternalError(format!("streaming tool execution task failed: {error}"))
            })?;
            self.mark_completed(&running_call);
            completed.push(StreamingToolExecutionOutput {
                stream_key: running_call.stream_key,
                output,
            });
        }
        if !completed.is_empty() {
            self.schedule_ready_calls().await;
        }
        Ok(completed)
    }

    pub async fn finish(&mut self) -> TalosResult<Vec<StreamingToolExecutionOutput>> {
        let mut completed = self.drain_completed().await?;
        loop {
            if self.pending_calls.is_empty() && self.parallel_running == 0 && !self.serial_running {
                break;
            }
            if self.parallel_running == 0 && !self.serial_running {
                self.schedule_ready_calls().await;
                continue;
            }
            let Some(result) = self.running_tasks.join_next().await else {
                break;
            };
            let (running_call, output) = result.map_err(|error| {
                TalosError::InternalError(format!("streaming tool execution task failed: {error}"))
            })?;
            self.mark_completed(&running_call);
            completed.push(StreamingToolExecutionOutput {
                stream_key: running_call.stream_key,
                output,
            });
            self.schedule_ready_calls().await;
            completed.extend(self.drain_completed().await?);
        }
        Ok(completed)
    }

    fn classify_call(&self, tool_name: &str) -> StreamingToolSchedulingClass {
        if self.execution_mode == ToolExecutionMode::Sequential {
            return StreamingToolSchedulingClass::SerialOnly;
        }
        if self.registry.tool_supports_parallel(tool_name)
            && !self.registry.tool_is_mutating(tool_name)
        {
            StreamingToolSchedulingClass::ParallelSafe
        } else {
            StreamingToolSchedulingClass::SerialOnly
        }
    }

    async fn schedule_ready_calls(&mut self) {
        loop {
            if self.serial_running {
                return;
            }

            let Some(next_call) = self.pending_calls.front().cloned() else {
                return;
            };

            match next_call.scheduling_class {
                StreamingToolSchedulingClass::SerialOnly => {
                    if self.parallel_running > 0 {
                        return;
                    }
                    let next_call = self
                        .pending_calls
                        .pop_front()
                        .expect("front item must exist when scheduling serial tool");
                    self.spawn_call(next_call).await;
                    return;
                }
                StreamingToolSchedulingClass::ParallelSafe => {
                    let next_call = self
                        .pending_calls
                        .pop_front()
                        .expect("front item must exist when scheduling parallel tool");
                    self.spawn_call(next_call).await;
                }
            }
        }
    }

    async fn spawn_call(&mut self, scheduled: ScheduledStreamingToolCall) {
        self.emit_executing_event(&scheduled).await;
        match scheduled.scheduling_class {
            StreamingToolSchedulingClass::ParallelSafe => {
                self.parallel_running += 1;
            }
            StreamingToolSchedulingClass::SerialOnly => {
                self.serial_running = true;
            }
        }

        let registry = self.registry.clone();
        let execution_context = self.execution_context.clone();
        let progress_emitter = self.progress_emitter(&scheduled);
        let running_call = RunningStreamingToolCall {
            stream_key: scheduled.stream_key.clone(),
            scheduling_class: scheduled.scheduling_class,
        };
        self.running_tasks.spawn(async move {
            let output = execute_single_tool_call(
                registry,
                &execution_context,
                scheduled.input,
                progress_emitter,
            )
            .await;
            (running_call, output)
        });
    }

    fn progress_emitter(
        &self,
        scheduled: &ScheduledStreamingToolCall,
    ) -> Option<ToolProgressEmitter> {
        let stream_tx = self.stream_tx.clone()?;
        let stream_key = scheduled.stream_key.clone();
        let call_id = scheduled.input.call_id.clone();
        let tool_name = scheduled.input.tool_name.clone();
        Some(ToolProgressEmitter::new(move |event: ToolProgressEvent| {
            let progress_kind = map_tool_progress_kind(event.kind);
            let _ = stream_tx.try_send(crate::KernelStreamEvent::new(
                crate::KernelStreamEventType::ToolProgressDelta,
                crate::KernelStreamEventData::ToolProgressDelta(
                    crate::ToolProgressDeltaStreamData {
                        stream_key: stream_key.clone(),
                        call_id: call_id.clone(),
                        tool_name: tool_name.clone(),
                        message: event.text,
                        progress_kind,
                    },
                ),
            ));
        }))
    }

    fn mark_completed(&mut self, running_call: &RunningStreamingToolCall) {
        match running_call.scheduling_class {
            StreamingToolSchedulingClass::ParallelSafe => {
                self.parallel_running = self.parallel_running.saturating_sub(1);
            }
            StreamingToolSchedulingClass::SerialOnly => {
                self.serial_running = false;
            }
        }
    }

    async fn emit_queued_event(&self, scheduled: &ScheduledStreamingToolCall) {
        emit_stream_event(
            self.stream_tx.as_ref(),
            crate::KernelStreamEventType::ToolQueued,
            crate::KernelStreamEventData::ToolQueued(crate::ToolQueuedStreamData {
                stream_key: scheduled.stream_key.clone(),
                call_id: scheduled.input.call_id.clone(),
                tool_name: scheduled.input.tool_name.clone(),
            }),
        )
        .await;
        if self.parallel_running > 0 || self.serial_running {
            emit_stream_event(
                self.stream_tx.as_ref(),
                crate::KernelStreamEventType::ToolProgressDelta,
                crate::KernelStreamEventData::ToolProgressDelta(
                    crate::ToolProgressDeltaStreamData {
                        stream_key: scheduled.stream_key.clone(),
                        call_id: scheduled.input.call_id.clone(),
                        tool_name: scheduled.input.tool_name.clone(),
                        message: "waiting for execution slot".to_string(),
                        progress_kind: crate::ToolProgressKind::Status,
                    },
                ),
            )
            .await;
        }
    }

    async fn emit_executing_event(&self, scheduled: &ScheduledStreamingToolCall) {
        emit_stream_event(
            self.stream_tx.as_ref(),
            crate::KernelStreamEventType::ToolExecuting,
            crate::KernelStreamEventData::ToolExecuting(crate::ToolExecutingStreamData {
                stream_key: scheduled.stream_key.clone(),
                call_id: scheduled.input.call_id.clone(),
                tool_name: scheduled.input.tool_name.clone(),
            }),
        )
        .await;
        emit_stream_event(
            self.stream_tx.as_ref(),
            crate::KernelStreamEventType::ToolProgressDelta,
            crate::KernelStreamEventData::ToolProgressDelta(crate::ToolProgressDeltaStreamData {
                stream_key: scheduled.stream_key.clone(),
                call_id: scheduled.input.call_id.clone(),
                tool_name: scheduled.input.tool_name.clone(),
                message: "execution started".to_string(),
                progress_kind: crate::ToolProgressKind::Status,
            }),
        )
        .await;
    }
}

async fn emit_stream_event(
    stream_tx: Option<&mpsc::Sender<crate::KernelStreamEvent>>,
    event_type: crate::KernelStreamEventType,
    data: crate::KernelStreamEventData,
) {
    if let Some(tx) = stream_tx {
        let _ = tx
            .send(crate::KernelStreamEvent::new(event_type, data))
            .await;
    }
}

fn map_tool_progress_kind(kind: talos_tools::ToolProgressKind) -> crate::ToolProgressKind {
    match kind {
        talos_tools::ToolProgressKind::Status => crate::ToolProgressKind::Status,
        talos_tools::ToolProgressKind::Stdout => crate::ToolProgressKind::Stdout,
        talos_tools::ToolProgressKind::Stderr => crate::ToolProgressKind::Stderr,
    }
}

#[cfg(test)]
#[path = "tool_execution_tests.rs"]
mod tests;
