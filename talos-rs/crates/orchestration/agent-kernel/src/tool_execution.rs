use crate::execution::ReplayDynamicToolResult;
use crate::ToolExecutionMode;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeSet, HashMap, VecDeque};
use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;
use talos_config::{AgentKernelConfig, TalosPaths};
use talos_error::{TalosError, TalosResult};
use talos_memory::MemoryScope;
use talos_run_control::{RunTaskCompletion, RunTaskGroup, SpawnError};
use talos_tools::{
    AbsolutePathBuf, ToolDispatchFailure, ToolDispatchOutcome, ToolDispatchResult,
    ToolExecutorPort, ToolInvocation, ToolProgressEmitter, ToolProgressEvent,
    ToolSandboxEnvironment,
};
use talos_types::UserActionResponse;
use tokio::sync::mpsc;
use tokio::task::JoinSet;

#[derive(Clone)]
pub struct ToolExecutionContext {
    pub trajectory: Option<Arc<dyn crate::trajectory::KernelTrajectoryRecorder>>,
    pub browser_uploads: Vec<talos_types::ToolInputFile>,
    pub session_id: String,
    pub turn_id: String,
    pub agent_id: String,
    pub workspace_id: String,
    pub user_id: String,
    pub project_root: Option<PathBuf>,
    pub kernel_config: Arc<AgentKernelConfig>,
    pub dynamic_tool_results: HashMap<String, ReplayDynamicToolResult>,
    pub user_action_responses: HashMap<String, UserActionResponse>,
    pub sandbox_approval_fingerprints: HashMap<String, String>,
    pub command_risk_assessor: Option<Arc<dyn talos_tools::CommandRiskAssessor>>,
    pub run_tasks: RunTaskGroup,
}

impl fmt::Debug for ToolExecutionContext {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ToolExecutionContext")
            .field("session_id", &self.session_id)
            .field("turn_id", &self.turn_id)
            .field("agent_id", &self.agent_id)
            .field("workspace_id", &self.workspace_id)
            .field("user_id", &self.user_id)
            .field("project_root", &self.project_root)
            .field(
                "has_command_risk_assessor",
                &self.command_risk_assessor.is_some(),
            )
            .finish_non_exhaustive()
    }
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
    #[serde(default)]
    pub arguments: Value,
    pub outcome: ToolDispatchOutcome,
    #[serde(default)]
    pub is_error: bool,
}

#[async_trait]
pub trait ToolExecutor: Send + Sync {
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
) -> TalosResult<talos_tools::ToolContext> {
    let mut workspace_roots = vec![PathBuf::from(
        &execution_context.kernel_config.workspace_dir,
    )];
    if let Some(project_root) = &execution_context.project_root {
        workspace_roots.push(project_root.clone());
    }
    let workspace_roots = workspace_roots
        .into_iter()
        .map(|root| {
            let root = std::fs::canonicalize(&root).map_err(|error| {
                let diagnostic = format!("resolve sandbox root {}: {error}", root.display());
                TalosError::internal_with_source(
                    "Resolve sandbox root; technical details were withheld.",
                    error,
                )
                .context(diagnostic)
            })?;
            AbsolutePathBuf::new(root).map_err(|error| {
                TalosError::internal_with_source(
                    "resolve sandbox root; technical details were withheld.",
                    error,
                )
            })
        })
        .collect::<TalosResult<Vec<_>>>()?;
    let readable_roots = resolve_runtime_readable_roots(&workspace_roots);
    let denied_roots = resolve_runtime_denied_roots()?;
    let sandbox_environment = ToolSandboxEnvironment::local(workspace_roots, denied_roots)
        .map_err(|error| {
            TalosError::internal_with_source(
                "resolve sandbox environment; technical details were withheld.",
                error,
            )
        })?
        .with_readable_roots(readable_roots);
    let ctx = registry
        .build_context(
            execution_context.session_id.clone(),
            execution_context.user_id.clone(),
            Some(execution_context.kernel_config.clone()),
        )
        .with_browser_uploads(execution_context.browser_uploads.clone())
        .with_turn_id(execution_context.turn_id.clone())
        .with_agent_id(execution_context.agent_id.clone())
        .with_sandbox_environment(sandbox_environment);
    let ctx = if let Some(project_root) = execution_context.project_root.clone() {
        ctx.with_project_root(project_root)
    } else {
        ctx
    };
    let ctx = ctx.with_memory_scope(MemoryScope {
        workspace_id: execution_context.workspace_id.clone(),
        user_id: execution_context.user_id.clone(),
        workspace_root: execution_context.kernel_config.workspace_dir.clone().into(),
    });
    let ctx = if let Some(assessor) = execution_context.command_risk_assessor.clone() {
        ctx.with_command_risk_assessor(assessor)
    } else {
        ctx
    };
    if let Some(result) = execution_context.dynamic_tool_results.get(call_id) {
        ctx.set_dynamic_tool_result(
            call_id.to_string(),
            talos_tools::handlers::dynamic::StoredDynamicToolResult {
                output: result.output.clone(),
                success: result.success,
            },
        );
    }
    if let Some(response) = execution_context.user_action_responses.get(call_id) {
        ctx.set_user_action_response(call_id.to_string(), response.clone());
    }
    if let Some(fingerprint) = execution_context.sandbox_approval_fingerprints.get(call_id) {
        ctx.set_sandbox_approval_fingerprint(call_id.to_string(), fingerprint.clone());
    }
    if let Some(emitter) = progress_emitter {
        Ok(ctx.with_progress_emitter(emitter))
    } else {
        Ok(ctx)
    }
}

fn resolve_runtime_denied_roots() -> TalosResult<Vec<AbsolutePathBuf>> {
    let env_file = TalosPaths::resolve().env_file;
    let env_file = if env_file.exists() {
        std::fs::canonicalize(&env_file).map_err(|error| {
            let diagnostic = format!("resolve protected env file {}: {error}", env_file.display());
            TalosError::internal_with_source(
                "Resolve protected env file; technical details were withheld.",
                error,
            )
            .context(diagnostic)
        })?
    } else {
        std::path::absolute(&env_file).map_err(|error| {
            let diagnostic = format!("resolve protected env file {}: {error}", env_file.display());
            TalosError::internal_with_source(
                "Resolve protected env file; technical details were withheld.",
                error,
            )
            .context(diagnostic)
        })?
    };
    AbsolutePathBuf::new(env_file)
        .map(|path| vec![path])
        .map_err(|error| {
            TalosError::internal_with_source(
                "resolve protected env file; technical details were withheld.",
                error,
            )
        })
}

fn resolve_runtime_readable_roots(workspace_roots: &[AbsolutePathBuf]) -> Vec<AbsolutePathBuf> {
    workspace_roots
        .iter()
        .filter_map(|path| path.as_path().ancestors().last())
        .filter_map(|path| AbsolutePathBuf::new(path.to_path_buf()).ok())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
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
    let result = match tool_context {
        Ok(tool_context) => match call.build_invocation(registry.as_ref()) {
            Ok(invocation) => {
                execute_tool_invocation(
                    registry,
                    &execution_context.run_tasks,
                    execution_context.trajectory.clone(),
                    call.call_id.clone(),
                    invocation,
                    tool_context,
                )
                .await
            }
            Err(error) => Err(error.into()),
        },
        Err(error) => Err(error.into()),
    };
    match result {
        Ok(payload) => ToolExecutionOutput {
            call_id: call.call_id,
            tool_name: call.tool_name,
            arguments: call.arguments,
            is_error: payload
                .payload()
                .map(|item| item.is_error())
                .unwrap_or(false),
            outcome: payload,
        },
        Err(error) => {
            talos_logging::report_error(error.error());
            ToolExecutionOutput {
                call_id: call.call_id,
                tool_name: call.tool_name,
                arguments: call.arguments,
                outcome: error.into_outcome(),
                is_error: true,
            }
        }
    }
}

async fn execute_tool_invocation(
    registry: Arc<dyn ToolExecutorPort>,
    run_tasks: &RunTaskGroup,
    trajectory: Option<Arc<dyn crate::trajectory::KernelTrajectoryRecorder>>,
    call_id: String,
    invocation: ToolInvocation,
    tool_context: talos_tools::ToolContext,
) -> ToolDispatchResult<ToolDispatchOutcome> {
    let run_task = run_tasks.spawn(move |cancellation| async move {
        let tool_context = tool_context.with_cancellation(
            talos_tools::InvocationCancellation::new(cancellation.requested()),
        );
        if let Some(observer) = &trajectory {
            observer.tool_started(&call_id);
        }
        let result = registry
            .execute_invocation_with_context(invocation, tool_context)
            .await;
        if let Some(observer) = &trajectory {
            observer.tool_finished(
                &call_id,
                if matches!(&result, Ok(ToolDispatchOutcome::Pending { .. })) {
                    crate::trajectory::KernelToolEnd::Waiting
                } else {
                    crate::trajectory::KernelToolEnd::Completed
                },
            );
        }
        if result.as_ref().is_err_and(|failure| {
            failure
                .error()
                .is_code(talos_error::ErrorCode::RunCleanupUnconfirmed)
        }) {
            RunTaskCompletion::CleanupUnconfirmed(result)
        } else {
            RunTaskCompletion::Confirmed(result)
        }
    });
    let run_task = run_task.map_err(|error| match error {
        SpawnError::Closed => ToolDispatchFailure::from(TalosError::new(
            talos_error::ErrorCode::Cancelled,
            "Run stopped accepting Tool tasks",
        )),
        SpawnError::CapacityExceeded => {
            ToolDispatchFailure::from(TalosError::internal("Run Tool task capacity exceeded"))
        }
    })?;
    run_task.await.map_err(|_| {
        ToolDispatchFailure::from(TalosError::new(
            talos_error::ErrorCode::Cancelled,
            "Tool task ended without a result",
        ))
    })?
}

#[async_trait]
impl ToolExecutor for SequentialToolExecutor {
    async fn execute(
        &self,
        execution_context: &ToolExecutionContext,
        registry: Arc<dyn ToolExecutorPort>,
        calls: &[ToolExecutionInput],
    ) -> TalosResult<Vec<ToolExecutionOutput>> {
        let mut outputs = Vec::with_capacity(calls.len());
        for call in calls {
            let output =
                execute_single_tool_call(registry.clone(), execution_context, call.clone(), None)
                    .await;
            let is_pending = output.outcome.pending().is_some();
            outputs.push(output);
            if is_pending {
                break;
            }
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
            let execution_context = execution_context.clone();
            join_set.spawn(async move {
                let output =
                    execute_single_tool_call(registry, &execution_context, call, None).await;
                (index, output)
            });
        }

        let mut outputs = vec![None; calls.len()];
        while let Some(result) = join_set.join_next().await {
            let (index, output) = result.map_err(|error| {
                TalosError::internal_with_source(
                    "parallel tool execution task failed; technical details were withheld.",
                    error,
                )
            })?;
            outputs[index] = Some(output);
        }
        outputs
            .into_iter()
            .enumerate()
            .map(|(index, output)| {
                output.ok_or_else(|| {
                    TalosError::internal(
                        "missing parallel tool output at index; technical details were withheld.",
                    )
                    .context(format!("missing parallel tool output at index {index}"))
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
    max_parallelism: usize,
    execution_context: ToolExecutionContext,
    registry: Arc<dyn ToolExecutorPort>,
    stream_tx: Option<mpsc::Sender<crate::AgentKernelEvent>>,
    pending_calls: VecDeque<ScheduledStreamingToolCall>,
    running_tasks: JoinSet<(RunningStreamingToolCall, ToolExecutionOutput)>,
    parallel_running: usize,
    serial_running: bool,
    halted: bool,
}

impl StreamingToolScheduler {
    pub fn new(
        execution_mode: ToolExecutionMode,
        max_parallelism: usize,
        execution_context: ToolExecutionContext,
        registry: Arc<dyn ToolExecutorPort>,
        stream_tx: Option<mpsc::Sender<crate::AgentKernelEvent>>,
    ) -> Self {
        let max_parallelism = match execution_mode {
            ToolExecutionMode::Sequential => 1,
            ToolExecutionMode::Parallel => max_parallelism.max(1),
        };
        Self {
            execution_mode,
            max_parallelism,
            execution_context,
            registry,
            stream_tx,
            pending_calls: VecDeque::new(),
            running_tasks: JoinSet::new(),
            parallel_running: 0,
            serial_running: false,
            halted: false,
        }
    }

    pub fn has_running(&self) -> bool {
        self.parallel_running > 0 || self.serial_running
    }

    pub async fn push_call(
        &mut self,
        stream_key: crate::ToolStreamKey,
        input: ToolExecutionInput,
    ) -> TalosResult<()> {
        if self.halted {
            return Ok(());
        }
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
                TalosError::internal_with_source(
                    "streaming tool execution task failed; technical details were withheld.",
                    error,
                )
            })?;
            self.mark_completed(&running_call);
            self.halt_if_pending(&output);
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

    pub async fn next_completed(&mut self) -> TalosResult<Option<StreamingToolExecutionOutput>> {
        let Some(result) = self.running_tasks.join_next().await else {
            return Ok(None);
        };
        let (running_call, output) = result.map_err(|error| {
            TalosError::internal_with_source(
                "streaming tool execution task failed; technical details were withheld.",
                error,
            )
        })?;
        self.mark_completed(&running_call);
        self.halt_if_pending(&output);
        if !self.halted {
            self.schedule_ready_calls().await;
        }
        Ok(Some(StreamingToolExecutionOutput {
            stream_key: running_call.stream_key,
            output,
        }))
    }

    pub async fn finish(&mut self) -> TalosResult<Vec<StreamingToolExecutionOutput>> {
        let mut completed = self.drain_completed().await?;
        loop {
            if (self.halted || self.pending_calls.is_empty())
                && self.parallel_running == 0
                && !self.serial_running
            {
                break;
            }
            if self.parallel_running == 0 && !self.serial_running {
                self.schedule_ready_calls().await;
                continue;
            }
            let Some(completed_output) = self.next_completed().await? else {
                break;
            };
            completed.push(completed_output);
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
        if self.halted {
            return;
        }
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
                    if self.parallel_running >= self.max_parallelism {
                        return;
                    }
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
            let _ = stream_tx.try_send(crate::AgentKernelEvent::new(
                crate::AgentKernelEventPayload::ToolProgressDelta(
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

    fn halt_if_pending(&mut self, output: &ToolExecutionOutput) {
        if output.outcome.pending().is_some() {
            self.halted = true;
        }
    }

    async fn emit_queued_event(&self, scheduled: &ScheduledStreamingToolCall) {
        if let Some(observer) = &self.execution_context.trajectory {
            observer.tool_queued(&scheduled.input.call_id);
        }
        emit_stream_event(
            self.stream_tx.as_ref(),
            crate::AgentKernelEventPayload::ToolQueued(crate::ToolQueuedStreamData {
                stream_key: scheduled.stream_key.clone(),
                call_id: scheduled.input.call_id.clone(),
                tool_name: scheduled.input.tool_name.clone(),
            }),
        )
        .await;
        if self.parallel_running > 0 || self.serial_running {
            emit_stream_event(
                self.stream_tx.as_ref(),
                crate::AgentKernelEventPayload::ToolProgressDelta(
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
            crate::AgentKernelEventPayload::ToolExecuting(crate::ToolExecutingStreamData {
                stream_key: scheduled.stream_key.clone(),
                call_id: scheduled.input.call_id.clone(),
                tool_name: scheduled.input.tool_name.clone(),
            }),
        )
        .await;
        emit_stream_event(
            self.stream_tx.as_ref(),
            crate::AgentKernelEventPayload::ToolProgressDelta(crate::ToolProgressDeltaStreamData {
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
    stream_tx: Option<&mpsc::Sender<crate::AgentKernelEvent>>,
    payload: crate::AgentKernelEventPayload,
) {
    if let Some(tx) = stream_tx {
        let _ = tx.send(crate::AgentKernelEvent::new(payload)).await;
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
