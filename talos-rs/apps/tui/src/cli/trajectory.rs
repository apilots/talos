//! Export the existing Turn trajectory and observed execution measurements.

use std::collections::{BTreeMap, HashMap};
use std::fs::File;
use std::io::Write;

use serde::Serialize;
use talos_sdk::trajectory::{TrajectoryReadTurnRequest, TrajectoryReadTurnResponse};
use talos_sdk::{
    AgentEvent, AgentEventPayload, AgentMessageBlock, AgentMessageBlockContent, AgentRunMetrics,
    AgentStopReason, AgentTurnRef, ContextCompactionMetrics, ContextCompactionOutcome, ErrorCode,
    GatewayClient,
};
use talos_types::talos::ToolExecutionMetrics;

#[derive(Default)]
pub(super) struct ExecObservation {
    pub(super) metrics: Option<AgentRunMetrics>,
    trace: Option<ExecTrace>,
    content: Option<Transcript>,
}

#[derive(Default)]
struct Transcript {
    positions: HashMap<String, u64>,
    completed: BTreeMap<u64, AgentMessageBlock>,
}

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct ExecTrace {
    identity: Option<AgentTurnRef>,
    settlement: Option<Settlement>,
    trajectory: Option<TrajectoryReadTurnResponse>,
    tools: Vec<ToolTiming>,
    compactions: Vec<Compaction>,
}

#[derive(Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
enum Settlement {
    Completed { stop_reason: AgentStopReason },
    Failed { code: ErrorCode },
    WaitingForInput,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ToolTiming {
    call_id: String,
    tool_name: String,
    is_error: bool,
    metrics: Option<ToolExecutionMetrics>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Compaction {
    outcome: Option<ContextCompactionOutcome>,
    metrics: Option<ContextCompactionMetrics>,
}

impl ExecObservation {
    pub(super) fn new(trace: bool, include_content: bool) -> Self {
        Self {
            trace: trace.then(ExecTrace::default),
            content: include_content.then(Transcript::default),
            ..Default::default()
        }
    }

    pub(super) fn record(&mut self, event: &AgentEvent) {
        if let AgentEventPayload::MetricsUpdated(metrics) = &event.payload {
            self.metrics = Some(metrics.clone());
        }
        let Some(trace) = self.trace.as_mut() else {
            return;
        };
        if let Some(transcript) = self.content.as_mut() {
            transcript.record(event);
        }
        trace.identity.get_or_insert_with(|| event.identity.clone());
        match &event.payload {
            AgentEventPayload::TurnCompleted(done) => {
                trace.settlement = Some(Settlement::Completed {
                    stop_reason: done.stop_reason,
                });
            }
            AgentEventPayload::TurnFailed(failed) => {
                trace.settlement = Some(Settlement::Failed {
                    code: failed.error.code,
                });
            }
            AgentEventPayload::WaitingForInput(_) => {
                trace.settlement = Some(Settlement::WaitingForInput);
            }
            AgentEventPayload::MessageBlockCompleted(block) => match &block.block.content {
                AgentMessageBlockContent::ToolResult(result) => trace.tools.push(ToolTiming {
                    call_id: result.call_id.clone(),
                    tool_name: result.tool_name.clone(),
                    is_error: result.is_error,
                    metrics: result.metrics.clone(),
                }),
                AgentMessageBlockContent::ContextCompaction(compaction) => {
                    trace.compactions.push(Compaction {
                        outcome: compaction.outcome,
                        metrics: compaction.metrics.clone(),
                    });
                }
                _ => {}
            },
            _ => {}
        }
    }

    pub(super) async fn write_trace(
        &mut self,
        client: &mut impl GatewayClient,
        mut file: File,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let Some(trace) = self.trace.as_mut() else {
            return Ok(());
        };
        let mut read_result = Ok(());
        if matches!(
            trace.settlement,
            Some(Settlement::Completed { .. } | Settlement::Failed { .. })
        ) {
            if let Some(identity) = trace.identity.as_ref() {
                match read_trajectory(client, identity).await {
                    Ok(trajectory) => trace.trajectory = Some(trajectory),
                    Err(error) => read_result = Err(error),
                }
            }
        }
        let mut document =
            serde_json::json!({"schemaVersion": 1, "metrics": self.metrics, "execution": trace});
        if let Some(transcript) = self.content.as_ref() {
            document["content"] =
                serde_json::to_value(transcript.completed.values().collect::<Vec<_>>())?;
        }
        serde_json::to_writer_pretty(&mut file, &document)?;
        writeln!(file)?;
        read_result.map_err(|error| error.to_api_error().message.into())
    }
}

impl Transcript {
    fn record(&mut self, event: &AgentEvent) {
        match &event.payload {
            AgentEventPayload::MessageBlockStarted(started)
                if matches!(
                    started.block.content,
                    AgentMessageBlockContent::UserContent(_)
                        | AgentMessageBlockContent::Reasoning(_)
                        | AgentMessageBlockContent::AssistantText(_)
                        | AgentMessageBlockContent::ToolCall(_)
                        | AgentMessageBlockContent::ToolResult(_)
                ) =>
            {
                self.positions
                    .insert(started.block.block_id.clone(), event.sequence);
            }
            AgentEventPayload::MessageBlockCompleted(completed) => {
                if let Some(sequence) = self.positions.remove(&completed.block.block_id) {
                    self.completed.insert(sequence, completed.block.clone());
                }
            }
            _ => {}
        }
    }
}

async fn read_trajectory(
    client: &mut impl GatewayClient,
    identity: &AgentTurnRef,
) -> talos_sdk::TalosResult<TrajectoryReadTurnResponse> {
    let mut first = client
        .read_turn_trajectory(TrajectoryReadTurnRequest {
            identity: identity.clone(),
            cursor: None,
            limit: None,
        })
        .await?;
    while let Some(cursor) = first.next_cursor.take() {
        let next = client
            .read_turn_trajectory(TrajectoryReadTurnRequest {
                identity: identity.clone(),
                cursor: Some(cursor),
                limit: None,
            })
            .await?;
        first.next_cursor = next.next_cursor;
        if let Some(entries) = next.data {
            first.data.get_or_insert_with(Vec::new).extend(entries);
        }
    }
    Ok(first)
}
