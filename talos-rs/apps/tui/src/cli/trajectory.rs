//! Export the existing Turn trajectory and observed execution measurements.

use std::collections::{BTreeMap, HashMap};
use std::fs::File;
use std::io::Write;

use serde::Serialize;
use talos_sdk::trajectory::{TrajectoryReadTurnRequest, TrajectoryReadTurnResponse};
use talos_sdk::{
    AgentEvent, AgentEventPayload, AgentMessageBlock, AgentMessageBlockContent, AgentRunMetrics,
    AgentTurnRef, GatewayClient,
};

#[derive(Default)]
pub(super) struct ExecObservation {
    pub(super) metrics: Option<AgentRunMetrics>,
    terminal: Option<AgentTurnRef>,
    content: Option<Transcript>,
}

#[derive(Default)]
struct Transcript {
    positions: HashMap<String, u64>,
    completed: BTreeMap<u64, AgentMessageBlock>,
}

impl ExecObservation {
    pub(super) fn new(include_content: bool) -> Self {
        Self {
            content: include_content.then(Transcript::default),
            ..Default::default()
        }
    }

    pub(super) fn record(&mut self, event: &AgentEvent) {
        match &event.payload {
            AgentEventPayload::MetricsUpdated(metrics) => self.metrics = Some(metrics.clone()),
            AgentEventPayload::TurnCompleted(_) | AgentEventPayload::TurnFailed(_) => {
                self.terminal = Some(event.identity.clone());
            }
            _ => {}
        }
        if let Some(transcript) = self.content.as_mut() {
            transcript.record(event);
        }
    }

    pub(super) async fn write_exports(
        &self,
        client: &mut impl GatewayClient,
        trajectory_file: File,
        transcript_file: Option<File>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if let (Some(file), Some(transcript)) = (transcript_file, &self.content) {
            write_json(file, &transcript.completed.values().collect::<Vec<_>>())?;
        }
        let trajectory = match &self.terminal {
            Some(identity) => Some(
                read_trajectory(client, identity)
                    .await
                    .map_err(|error| error.to_api_error().message)?,
            ),
            None => None,
        };
        write_json(trajectory_file, &trajectory)
    }
}

fn write_json(mut file: File, value: &impl Serialize) -> Result<(), Box<dyn std::error::Error>> {
    serde_json::to_writer_pretty(&mut file, value)?;
    writeln!(file)?;
    Ok(())
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
