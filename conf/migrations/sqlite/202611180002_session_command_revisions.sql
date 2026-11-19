-- Commands can complete in a transaction that appends no Timeline event.
CREATE TABLE session_command_revisions (
    revision INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL,
    owner_user_id TEXT NOT NULL,
    history_generation INTEGER NOT NULL,
    command TEXT NOT NULL CHECK (command IN ('turn_submission', 'turn_input')),
    idempotency_key TEXT NOT NULL,
    request_id TEXT NOT NULL,
    request_hash TEXT NOT NULL,
    turn_id TEXT NOT NULL,
    message_id TEXT,
    outcome TEXT NOT NULL CHECK (outcome IN ('accepted', 'failed', 'failed_no_first_event')),
    committed_event_seq INTEGER NOT NULL,
    record_expires_at INTEGER
);
CREATE INDEX session_command_revisions_order ON session_command_revisions(session_id, history_generation, revision);

INSERT INTO session_command_revisions (
    session_id, owner_user_id, history_generation, command, idempotency_key, request_id,
    request_hash, turn_id, message_id, outcome, committed_event_seq, record_expires_at
)
SELECT i.session_id, s.user_id, s.history_generation, i.command, i.idempotency_key, i.request_id,
       i.request_hash, i.turn_id, i.message_id, i.outcome, i.committed_event_seq, i.record_expires_at
FROM agent_command_idempotency i
JOIN sessions s ON s.session_key = i.session_id
JOIN agent_events e ON e.event_seq = i.committed_event_seq
                  AND e.session_id = s.session_key AND e.history_generation = s.history_generation
WHERE i.status = 'completed'
ORDER BY i.committed_event_seq, i.rowid;
