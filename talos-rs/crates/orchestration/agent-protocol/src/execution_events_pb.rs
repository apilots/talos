//! Typed bounded projection of committed Worker execution events.
use crate::{
    harness_pb, AgentCommittedEvent, AgentExecutionSettlement, AgentSessionEventCursor,
    AgentSessionEventsResult, MAX_EXECUTION_PAGE_BYTES, MAX_EXECUTION_PAGE_EVENTS,
};
use agent_pb as pb;
use prost::Message;
use talos_error::{ErrorCode, TalosError, TalosResult};

pub fn encode_cursor(value: AgentSessionEventCursor) -> pb::AgentSessionEventCursor {
    pb::AgentSessionEventCursor {
        session_id: value.session_id,
        generation: value.generation,
        offset: value.offset,
    }
}
pub fn decode_cursor(value: pb::AgentSessionEventCursor) -> TalosResult<AgentSessionEventCursor> {
    if value.session_id.is_empty() || value.offset < 0 {
        return Err(invalid());
    }
    Ok(AgentSessionEventCursor {
        session_id: value.session_id,
        generation: value.generation,
        offset: value.offset,
    })
}
pub fn encode_page(value: AgentSessionEventsResult) -> TalosResult<pb::AgentSessionEventsPage> {
    let events = value
        .events
        .into_iter()
        .map(|record| {
            Ok(pb::AgentCommittedEvent {
                execution_purpose: match record.execution_purpose {
                    crate::AgentExecutionPurpose::Turn => 1,
                    crate::AgentExecutionPurpose::Compaction => 2,
                },
                history_supplement: record
                    .history_supplement
                    .as_ref()
                    .map(encode_capture)
                    .transpose()?
                    .unwrap_or_default(),
                execution_model: record.execution_model.map(|model| pb::ModelSelection {
                    provider: model.provider,
                    model_id: model.id,
                    reasoning_effort: None,
                }),
                cursor: Some(encode_cursor(record.cursor)),
                automation_input: record
                    .automation_input
                    .map(crate::automation_pb::encode_input_origin),
                command: record.command.map(harness_pb::encode_command),
                event: Some(harness_pb::encode_event(record.event)?),
                settlement: record
                    .settlement
                    .map(|settlement| pb::AgentExecutionSettlement {
                        state: Some(match settlement {
                            AgentExecutionSettlement::WaitingForInput { metrics } => {
                                pb::agent_execution_settlement::State::Waiting(Box::new(
                                    pb::WaitingSettlement {
                                        event: None,
                                        metrics: Some(harness_pb::encode_metrics(metrics)),
                                    },
                                ))
                            }
                            AgentExecutionSettlement::Finished {
                                metrics,
                                memory_usage,
                            } => pb::agent_execution_settlement::State::Finished(Box::new(
                                pb::AgentFinishedSettlement {
                                    metrics: Some(harness_pb::encode_metrics(metrics)),
                                    memory_usage: memory_usage
                                        .into_iter()
                                        .map(encode_memory_usage)
                                        .collect(),
                                },
                            )),
                        }),
                    }),
            })
        })
        .collect::<TalosResult<Vec<_>>>()?;
    let page = pb::AgentSessionEventsPage {
        events,
        next_cursor: Some(encode_cursor(value.next_cursor)),
        has_more: value.has_more,
        active_turn: value.active_turn.map(harness_pb::encode_turn_ref),
    };
    validate_size(&page)?;
    Ok(page)
}
pub fn decode_page(value: pb::AgentSessionEventsPage) -> TalosResult<AgentSessionEventsResult> {
    validate_size(&value)?;
    let next_cursor = decode_cursor(value.next_cursor.ok_or_else(invalid)?)?;
    let events = value
        .events
        .into_iter()
        .map(|record| {
            let cursor = decode_cursor(record.cursor.ok_or_else(invalid)?)?;
            let event = harness_pb::decode_event(record.event.ok_or_else(invalid)?)?;
            if cursor.session_id != next_cursor.session_id
                || cursor.generation != next_cursor.generation
                || cursor.session_id != event.identity.session_id
                || cursor.offset > next_cursor.offset
            {
                return Err(invalid());
            }
            let settlement = record
                .settlement
                .map(|settlement| -> TalosResult<AgentExecutionSettlement> {
                    match settlement.state.ok_or_else(invalid)? {
                        pb::agent_execution_settlement::State::Waiting(waiting) => {
                            Ok(AgentExecutionSettlement::WaitingForInput {
                                metrics: harness_pb::decode_metrics(waiting.metrics)?,
                            })
                        }
                        pb::agent_execution_settlement::State::Finished(finished) => {
                            Ok(AgentExecutionSettlement::Finished {
                                memory_usage: finished
                                    .memory_usage
                                    .into_iter()
                                    .map(decode_memory_usage)
                                    .collect::<TalosResult<Vec<_>>>()?,
                                metrics: harness_pb::decode_metrics(finished.metrics)?,
                            })
                        }
                    }
                })
                .transpose()?;
            let history_supplement = decode_capture(&record.history_supplement)?;
            if history_supplement.as_ref().is_some_and(|capture| {
                capture.identity != event.identity || !event.payload.is_terminal()
            }) {
                return Err(invalid());
            }
            Ok(AgentCommittedEvent {
                execution_purpose: match record.execution_purpose {
                    1 => crate::AgentExecutionPurpose::Turn,
                    2 => crate::AgentExecutionPurpose::Compaction,
                    _ => return Err(invalid()),
                },
                history_supplement,
                execution_model: record
                    .execution_model
                    .map(|model| {
                        if model.provider.is_empty() || model.model_id.is_empty() {
                            return Err(invalid());
                        }
                        Ok(talos_types::ModelRef {
                            provider: model.provider,
                            id: model.model_id,
                        })
                    })
                    .transpose()?,
                cursor,
                event,
                settlement,
                automation_input: record
                    .automation_input
                    .map(crate::automation_pb::decode_input_origin)
                    .transpose()?,
                command: record.command.map(harness_pb::decode_command).transpose()?,
            })
        })
        .collect::<TalosResult<Vec<_>>>()?;
    if events
        .windows(2)
        .any(|pair| pair[0].cursor.offset >= pair[1].cursor.offset)
    {
        return Err(invalid());
    }
    Ok(AgentSessionEventsResult {
        events,
        next_cursor,
        has_more: value.has_more,
        active_turn: value
            .active_turn
            .map(|value| harness_pb::decode_turn_ref(Some(value)))
            .transpose()?,
    })
}
fn validate_size(page: &pb::AgentSessionEventsPage) -> TalosResult<()> {
    if page.events.len() > MAX_EXECUTION_PAGE_EVENTS
        || page.encoded_len() > MAX_EXECUTION_PAGE_BYTES
    {
        return Err(invalid());
    }
    Ok(())
}
fn invalid() -> TalosError {
    TalosError::new(
        ErrorCode::InvalidRequest,
        "Worker execution replay payload is invalid or exceeds its bounded limit.",
    )
}

fn encode_memory_usage(value: crate::AgentMemoryModelUsage) -> pb::AgentMemoryModelUsage {
    pb::AgentMemoryModelUsage {
        request_id: value.request_id,
        model_ref: Some(pb::ModelSelection {
            provider: value.model_ref.provider,
            model_id: value.model_ref.id,
            reasoning_effort: None,
        }),
        usage: Some(
            harness_pb::encode_metrics(talos_agent_api::AgentRunMetrics {
                usage: value.usage,
                ..Default::default()
            })
            .usage
            .expect("metrics encodes usage"),
        ),
        completed_at: value.completed_at,
        timezone: value.timezone,
    }
}
fn decode_memory_usage(
    value: pb::AgentMemoryModelUsage,
) -> TalosResult<crate::AgentMemoryModelUsage> {
    let model = value.model_ref.ok_or_else(invalid)?;
    if value.request_id.is_empty()
        || model.provider.is_empty()
        || model.model_id.is_empty()
        || value.timezone.is_empty()
    {
        return Err(invalid());
    }
    let metrics = harness_pb::decode_metrics(Some(pb::RunMetrics {
        usage: value.usage,
        ..Default::default()
    }))?;
    Ok(crate::AgentMemoryModelUsage {
        request_id: value.request_id,
        model_ref: talos_types::ModelRef {
            provider: model.provider,
            id: model.model_id,
        },
        usage: metrics.usage,
        completed_at: value.completed_at,
        timezone: value.timezone,
    })
}

fn encode_capture(
    value: &talos_agent_api::trajectory::SessionHistorySupplement,
) -> TalosResult<Vec<u8>> {
    if !value.payloads.is_empty() {
        return Err(invalid());
    }
    let bytes = serde_json::to_vec(value).map_err(|error| {
        TalosError::internal_with_source("Encode sealed execution capture.", error)
    })?;
    if bytes.len() > crate::MAX_EXECUTION_CAPTURE_BYTES {
        return Err(invalid());
    }
    Ok(bytes)
}
fn decode_capture(
    bytes: &[u8],
) -> TalosResult<Option<talos_agent_api::trajectory::SessionHistorySupplement>> {
    if bytes.is_empty() {
        return Ok(None);
    }
    if bytes.len() > crate::MAX_EXECUTION_CAPTURE_BYTES {
        return Err(invalid());
    }
    let value: talos_agent_api::trajectory::SessionHistorySupplement =
        serde_json::from_slice(bytes).map_err(|error| {
            TalosError::from_source(
                ErrorCode::InvalidRequest,
                "Sealed execution capture is invalid.",
                error,
            )
        })?;
    if !value.payloads.is_empty() {
        return Err(invalid());
    }
    Ok(Some(value))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_replay_preserves_model_command_and_background_memory_usage() {
        let model = talos_types::ModelRef {
            provider: "test".into(),
            id: "model".into(),
        };
        let identity = talos_agent_api::AgentTurnRef::new("session", "turn");
        let first = AgentCommittedEvent {
            execution_purpose: crate::AgentExecutionPurpose::Turn,
            history_supplement: None,
            execution_model: Some(model.clone()),
            cursor: AgentSessionEventCursor {
                session_id: "session".into(),
                generation: 2,
                offset: 10,
            },
            event: talos_agent_api::AgentEvent {
                identity: identity.clone(),
                sequence: 5,
                occurred_at: 1,
                payload: talos_agent_api::AgentEventPayload::MessageCompleted(
                    talos_agent_api::AgentMessage {
                        message_id: "input".into(),
                        turn_id: "turn".into(),
                        role: talos_agent_api::AgentMessageRole::User,
                        status: talos_agent_api::AgentMessageStatus::Completed,
                        blocks: Vec::new(),
                        created_at: 1,
                        completed_at: Some(1),
                    },
                ),
            },
            command: Some(crate::AgentCommandIdentity {
                command: talos_types::IdempotentCommand::TurnSubmission,
                idempotency_key: "command-key".into(),
                request_id: "request-id".into(),
            }),
            automation_input: None,
            settlement: None,
        };
        let final_fact = AgentCommittedEvent {
            execution_purpose: crate::AgentExecutionPurpose::Turn,
            history_supplement: None,
            execution_model: None,
            cursor: AgentSessionEventCursor {
                session_id: "session".into(),
                generation: 2,
                offset: 11,
            },
            event: talos_agent_api::AgentEvent {
                identity,
                sequence: 6,
                occurred_at: 2,
                payload: talos_agent_api::AgentEventPayload::TurnCompleted(
                    talos_agent_api::AgentTurnCompleted {
                        stop_reason: talos_agent_api::AgentStopReason::EndTurn,
                    },
                ),
            },
            command: None,
            automation_input: None,
            settlement: Some(AgentExecutionSettlement::Finished {
                metrics: Default::default(),
                memory_usage: vec![crate::AgentMemoryModelUsage {
                    request_id: "memory-request".into(),
                    model_ref: model,
                    usage: talos_types::TokenUsage {
                        input_tokens: 4,
                        output_tokens: 2,
                        total_tokens: 6,
                        ..Default::default()
                    },
                    completed_at: 2,
                    timezone: "UTC".into(),
                }],
            }),
        };
        let page = AgentSessionEventsResult {
            next_cursor: final_fact.cursor.clone(),
            events: vec![first, final_fact],
            active_turn: None,
            has_more: false,
        };
        assert_eq!(
            decode_page(encode_page(page.clone()).unwrap()).unwrap(),
            page
        );
    }

    #[test]
    fn replay_rejects_a_cursor_from_another_session() {
        let page = pb::AgentSessionEventsPage {
            events: vec![pb::AgentCommittedEvent {
                execution_purpose: 1,
                history_supplement: Vec::new(),
                cursor: Some(pb::AgentSessionEventCursor {
                    session_id: "other".into(),
                    generation: 0,
                    offset: 1,
                }),
                event: Some(
                    harness_pb::encode_event(talos_agent_api::AgentEvent {
                        identity: talos_agent_api::AgentTurnRef::new("session", "turn"),
                        sequence: 1,
                        occurred_at: 1,
                        payload: talos_agent_api::AgentEventPayload::TurnStarted(
                            talos_agent_api::AgentTurnStarted {},
                        ),
                    })
                    .unwrap(),
                ),
                ..Default::default()
            }],
            next_cursor: Some(pb::AgentSessionEventCursor {
                session_id: "session".into(),
                generation: 0,
                offset: 1,
            }),
            has_more: false,
            active_turn: None,
        };
        assert!(decode_page(page).is_err());
    }
}
