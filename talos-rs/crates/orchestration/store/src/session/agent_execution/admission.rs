use talos_agent_api::{ActiveTurnState, AgentEvent};
use talos_error::TalosResult;

use super::record::{append_events, load_active_turn, load_event_sequence};
use super::{
    execution_conflict, validate_initial_admission, validate_progress_event,
    validate_user_admission, AgentActiveTurnRecord, AgentEventCommit, CommitInitialAdmissionParams,
    CommitRunningInputAdmissionParams, IdempotentCommand,
};
use crate::session::DatabaseSessionStore;

#[derive(Clone, Copy)]
enum RunningCommitKind {
    Progress,
    Input,
}

impl RunningCommitKind {
    fn accepts(self, state: ActiveTurnState) -> bool {
        match self {
            Self::Progress => matches!(
                state,
                ActiveTurnState::Running | ActiveTurnState::Cancelling
            ),
            Self::Input => state == ActiveTurnState::Running,
        }
    }
}

pub(super) async fn commit_initial(
    store: &DatabaseSessionStore,
    params: CommitInitialAdmissionParams,
) -> TalosResult<AgentEventCommit> {
    let CommitInitialAdmissionParams { events, command } = params;
    let (identity, initial_message_id) = validate_initial_admission(&events)?;
    if let Some(command) = command.as_ref() {
        if command.command != IdempotentCommand::TurnSubmission
            || command.session_id != identity.session_id
            || command.turn_id != identity.turn_id
            || command.message_id != initial_message_id
        {
            return Err(execution_conflict(
                "Turn admission does not match its idempotency lease",
            ));
        }
    }
    let mut transaction = store
        .begin_session_transaction(&identity.session_id)
        .await?;
    ensure_registration_retained(transaction.as_mut(), &identity.session_id).await?;
    let records = append_events(transaction.as_mut(), events).await?;
    let last_event_seq = records
        .last()
        .expect("validated admission is non-empty")
        .event_seq;
    let updated_at = chrono::Utc::now().timestamp();
    let last_event_seq_param = last_event_seq.to_string();
    let updated_at_param = updated_at.to_string();
    transaction
        .execute(
            r#"
            INSERT INTO agent_active_turns (
                session_id, turn_id, initial_message_id, state,
                last_event_seq, updated_at
            ) VALUES (?, ?, ?, 'running', ?, ?)
            "#,
            &[
                identity.session_id.as_str(),
                identity.turn_id.as_str(),
                initial_message_id.as_str(),
                last_event_seq_param.as_str(),
                updated_at_param.as_str(),
            ],
        )
        .await?;
    if let Some(command) = command.as_ref() {
        super::idempotency::complete_command(
            transaction.as_mut(),
            command,
            IdempotentCommand::TurnSubmission,
            last_event_seq,
            chrono::Utc::now().timestamp_millis(),
        )
        .await?;
    }
    transaction.commit().await?;
    Ok(AgentEventCommit {
        events: records,
        active_turn: AgentActiveTurnRecord {
            identity,
            initial_message_id: Some(initial_message_id),
            state: ActiveTurnState::Running,
            last_event_seq,
            current_action_id: None,
            updated_at,
        },
    })
}

pub(super) async fn commit_maintenance(
    store: &DatabaseSessionStore,
    event: AgentEvent,
) -> TalosResult<AgentEventCommit> {
    if event.sequence != 1
        || !matches!(
            event.payload,
            talos_agent_api::AgentEventPayload::TurnStarted(_)
        )
    {
        return Err(execution_conflict(
            "maintenance admission requires only TurnStarted at sequence 1",
        ));
    }
    let identity = event.identity.clone();
    let mut transaction = store
        .begin_session_transaction(&identity.session_id)
        .await?;
    ensure_registration_retained(transaction.as_mut(), &identity.session_id).await?;
    let mut records = append_events(transaction.as_mut(), vec![event]).await?;
    let record = records
        .pop()
        .expect("one maintenance admission produces one record");
    let updated_at = chrono::Utc::now().timestamp();
    let last_event_seq = record.event_seq.to_string();
    let updated_at_param = updated_at.to_string();
    transaction
        .execute(
            r#"
            INSERT INTO agent_active_turns (
                session_id, turn_id, initial_message_id, state,
                last_event_seq, updated_at
            ) VALUES (?, ?, NULL, 'running', ?, ?)
            "#,
            &[
                identity.session_id.as_str(),
                identity.turn_id.as_str(),
                last_event_seq.as_str(),
                updated_at_param.as_str(),
            ],
        )
        .await?;
    transaction.commit().await?;
    Ok(AgentEventCommit {
        events: vec![record.clone()],
        active_turn: AgentActiveTurnRecord {
            identity,
            initial_message_id: None,
            state: ActiveTurnState::Running,
            last_event_seq: record.event_seq,
            current_action_id: None,
            updated_at,
        },
    })
}

pub(super) async fn commit_progress(
    store: &DatabaseSessionStore,
    event: AgentEvent,
    expected_event_seq: i64,
) -> TalosResult<AgentEventCommit> {
    commit_progress_batch(store, vec![event], expected_event_seq).await
}

pub(super) async fn commit_progress_batch(
    store: &DatabaseSessionStore,
    events: Vec<AgentEvent>,
    expected_event_seq: i64,
) -> TalosResult<AgentEventCommit> {
    super::validate_event_batch(&events)?;
    for event in &events {
        validate_progress_event(event)?;
    }
    commit_running_batch(
        store,
        events,
        expected_event_seq,
        RunningCommitKind::Progress,
        None,
    )
    .await
}

pub(super) async fn commit_running_input(
    store: &DatabaseSessionStore,
    params: CommitRunningInputAdmissionParams,
) -> TalosResult<AgentEventCommit> {
    let CommitRunningInputAdmissionParams {
        events,
        expected_event_seq,
        command,
    } = params;
    let (identity, message_id) = validate_user_admission(&events)?;
    validate_input_command(command.as_ref(), &identity, &message_id)?;
    commit_running_batch(
        store,
        events,
        expected_event_seq,
        RunningCommitKind::Input,
        command,
    )
    .await
}

async fn commit_running_batch(
    store: &DatabaseSessionStore,
    events: Vec<AgentEvent>,
    expected_event_seq: i64,
    kind: RunningCommitKind,
    command: Option<super::CommandLease>,
) -> TalosResult<AgentEventCommit> {
    let identity = events
        .first()
        .expect("validated Agent event batch is non-empty")
        .identity
        .clone();
    let mut transaction = store
        .begin_session_transaction(&identity.session_id)
        .await?;
    let active = load_active_turn(transaction.as_mut(), &identity.session_id)
        .await?
        .ok_or_else(|| execution_conflict("active Agent Turn does not exist"))?;
    if active.identity != identity
        || !kind.accepts(active.state)
        || active.last_event_seq != expected_event_seq
    {
        return Err(execution_conflict(
            "active Agent Turn changed before running event commit",
        ));
    }
    let last_sequence = load_event_sequence(transaction.as_mut(), expected_event_seq).await?;
    if events[0].sequence != last_sequence.saturating_add(1) {
        return Err(execution_conflict(
            "Agent running event sequence is not contiguous",
        ));
    }
    let records = append_events(transaction.as_mut(), events).await?;
    let last_event_seq = records
        .last()
        .expect("validated Agent event batch is non-empty")
        .event_seq;
    let updated_at = chrono::Utc::now().timestamp();
    let last_event_seq_param = last_event_seq.to_string();
    let updated_at_param = updated_at.to_string();
    let expected_param = expected_event_seq.to_string();
    let expected_state = match active.state {
        ActiveTurnState::Running => "running",
        ActiveTurnState::WaitingForInput => "waiting_for_input",
        ActiveTurnState::Cancelling => "cancelling",
    };
    let affected = transaction
        .execute(
            r#"
            UPDATE agent_active_turns
            SET last_event_seq = ?, updated_at = ?
            WHERE session_id = ? AND turn_id = ?
              AND state = ? AND last_event_seq = ?
            "#,
            &[
                last_event_seq_param.as_str(),
                updated_at_param.as_str(),
                identity.session_id.as_str(),
                identity.turn_id.as_str(),
                expected_state,
                expected_param.as_str(),
            ],
        )
        .await?;
    if affected != 1 {
        return Err(execution_conflict(
            "active Agent Turn tail changed during running event commit",
        ));
    }
    if let Some(command) = command.as_ref() {
        super::idempotency::complete_command(
            transaction.as_mut(),
            command,
            IdempotentCommand::TurnInput,
            last_event_seq,
            chrono::Utc::now().timestamp_millis(),
        )
        .await?;
    }
    transaction.commit().await?;
    Ok(AgentEventCommit {
        events: records,
        active_turn: AgentActiveTurnRecord {
            last_event_seq,
            updated_at,
            ..active
        },
    })
}

fn validate_input_command(
    command: Option<&super::CommandLease>,
    identity: &talos_agent_api::AgentTurnRef,
    message_id: &str,
) -> TalosResult<()> {
    if command.is_some_and(|command| {
        command.command != IdempotentCommand::TurnInput
            || command.session_id != identity.session_id
            || command.turn_id != identity.turn_id
            || command.message_id != message_id
    }) {
        return Err(execution_conflict(
            "running input does not match its idempotency lease",
        ));
    }
    Ok(())
}

async fn ensure_registration_retained(
    tx: &mut dyn talos_storage::StorageTransaction,
    session_id: &str,
) -> TalosResult<()> {
    if !tx.query("SELECT 1 FROM sessions s JOIN computer_deletions d ON d.node_id = s.execution_node_id WHERE s.session_key = ?", &[session_id]).await?.is_empty() {
        return Err(talos_error::TalosError::new(talos_error::ErrorCode::ResourceExpired,
            "This Computer registration was removed. Select a connected Computer to start new work."));
    }
    Ok(())
}
