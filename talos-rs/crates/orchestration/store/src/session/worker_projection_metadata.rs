//! Product-owned projections of admission provenance and waiting state.
use super::{conflict, encode};
use crate::session::AgentEventRecord;
use agent_protocol::AgentCommittedEvent;
use talos_agent_api::{AgentEventPayload, AgentMessageRole};
use talos_error::TalosResult;
use talos_storage::StorageTransaction;

pub(super) async fn project(
    tx: &mut dyn StorageTransaction,
    fact: &AgentCommittedEvent,
    record: &AgentEventRecord,
) -> TalosResult<()> {
    let event = &fact.event;
    if let Some(supplement) = &fact.history_supplement {
        if !event.payload.is_terminal()
            || supplement.identity != event.identity
            || !supplement.payloads.is_empty()
        {
            return Err(conflict(
                "Worker trajectory metadata must belong to its completed Turn",
            ));
        }
        // The Worker seals this metadata with its terminal fact. Large payloads stay
        // in the execution store and use the bounded Worker payload read contract.
        let affected = tx.execute(
            "UPDATE agent_events SET execution_capture_json=? WHERE session_id=? AND turn_id=? AND sequence=1 AND json_extract(payload_json,'$.type')='turnStarted'",
            &[&encode(supplement)?, &event.identity.session_id, &event.identity.turn_id],
        ).await?;
        if affected != 1 {
            return Err(conflict("Worker trajectory metadata has no original Turn"));
        }
    }
    if let Some(origin) = &fact.automation_input {
        if !matches!(&event.payload, AgentEventPayload::MessageCompleted(message) if message.role==AgentMessageRole::User && message.message_id==origin.message_id)
        {
            return Err(conflict(
                "Automation provenance does not match the committed Input",
            ));
        }
        crate::automation::goal_loop::bind_replayed_admission_origin(
            tx,
            &event.identity,
            &origin.origin,
            &origin.message_id,
        )
        .await?;
    }
    if let Some(command) = &fact.command {
        let AgentEventPayload::MessageCompleted(message) = &event.payload else {
            return Err(conflict("Command provenance requires a completed Input"));
        };
        if message.role != AgentMessageRole::User {
            return Err(conflict("Command provenance requires User input"));
        }
        let affected = tx.execute("UPDATE agent_command_idempotency SET status='completed',outcome='accepted',committed_event_seq=?,lease_id=NULL,lease_expires_at=NULL,updated_at=? WHERE session_id=? AND command=? AND idempotency_key=? AND request_id=? AND turn_id=? AND message_id=? AND status='reserved'", &[&record.event_seq.to_string(),&chrono::Utc::now().timestamp_millis().to_string(),&event.identity.session_id,command.command.as_str(),&command.idempotency_key,&command.request_id,&event.identity.turn_id,&message.message_id]).await?;
        if affected == 1 {
            super::super::agent_execution::idempotency::record_completion(
                tx,
                &event.identity.session_id,
                command.command,
                &command.idempotency_key,
            )
            .await?;
        }
    }
    Ok(())
}

/// Restore canonical provenance after source-fingerprint validation without
/// rebinding admission controls or completing any command receipt again.
pub(super) async fn restore(
    tx: &mut dyn StorageTransaction,
    fact: &AgentCommittedEvent,
) -> TalosResult<()> {
    let identity = &fact.event.identity;
    if let Some(supplement) = &fact.history_supplement {
        tx.execute("UPDATE agent_events SET execution_capture_json=COALESCE(execution_capture_json,?) WHERE session_id=? AND turn_id=? AND sequence=1 AND history_generation=?", &[&encode(supplement)?, &identity.session_id, &identity.turn_id, &fact.cursor.generation.to_string()]).await?;
    }
    if let Some(input) = &fact.automation_input {
        tx.execute("UPDATE agent_events SET automation_origin_json=COALESCE(automation_origin_json,?),automation_input_message_id=COALESCE(automation_input_message_id,?) WHERE session_id=? AND turn_id=? AND sequence=1 AND history_generation=?", &[&encode(&input.origin)?, &input.message_id, &identity.session_id, &identity.turn_id, &fact.cursor.generation.to_string()]).await?;
    }
    Ok(())
}
