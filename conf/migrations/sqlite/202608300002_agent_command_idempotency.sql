-- Replaces response blobs with normalized command identity and lease ownership.
CREATE TABLE agent_command_idempotency (
    session_id TEXT NOT NULL,
    command TEXT NOT NULL CHECK (command IN ('turn_submission', 'turn_input')),
    idempotency_key TEXT NOT NULL,
    request_hash TEXT NOT NULL,
    request_id TEXT NOT NULL,
    turn_id TEXT NOT NULL,
    message_id TEXT,
    status TEXT NOT NULL CHECK (status IN ('reserved', 'completed')),
    outcome TEXT CHECK (outcome IN ('accepted', 'failed', 'failed_no_first_event')),
    committed_event_seq INTEGER,
    lease_id TEXT,
    lease_expires_at INTEGER,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    record_expires_at INTEGER,
    PRIMARY KEY (session_id, command, idempotency_key),
    CHECK (
        (status = 'reserved' AND length(lease_id) > 0 AND lease_expires_at > 0 AND outcome IS NULL)
        OR (status = 'completed' AND lease_id IS NULL AND lease_expires_at IS NULL AND outcome IS NOT NULL)
    )
);

INSERT INTO agent_command_idempotency (
    session_id, command, idempotency_key, request_hash, request_id,
    turn_id, message_id, status, outcome, committed_event_seq,
    lease_id, lease_expires_at, created_at, updated_at, record_expires_at
)
SELECT
    session_id,
    'turn_submission',
    idempotency_key,
    request_hash,
    COALESCE(NULLIF(json_extract(result_json, '$.request_id'), ''), 'req_legacy_' || rowid),
    turn_id,
    COALESCE(NULLIF(message_id, ''), NULLIF(json_extract(result_json, '$.message_id'), '')),
    CASE WHEN status = 'pending' THEN 'reserved' ELSE 'completed' END,
    CASE
        WHEN status = 'accepted' THEN 'accepted'
        WHEN status = 'failed' THEN 'failed'
        WHEN status = 'failed_no_first_event' THEN 'failed_no_first_event'
        ELSE NULL
    END,
    (
        SELECT event_seq FROM agent_events
        WHERE agent_events.session_id = message_idempotency_keys.session_id
          AND agent_events.turn_id = message_idempotency_keys.turn_id
          AND sequence = 1
        LIMIT 1
    ),
    CASE WHEN status = 'pending' THEN 'lease_legacy_' || rowid ELSE NULL END,
    CASE WHEN status = 'pending' THEN 1 ELSE NULL END,
    created_at,
    updated_at,
    NULLIF(expires_at, 0)
FROM message_idempotency_keys;

DROP TRIGGER IF EXISTS sessions_cleanup_after_delete;
DROP TABLE message_idempotency_keys;

CREATE TRIGGER sessions_cleanup_after_delete
AFTER DELETE ON sessions
BEGIN
    DELETE FROM timeline_events WHERE session_id = OLD.session_key;
    DELETE FROM active_turns WHERE session_id = OLD.session_key;
    DELETE FROM agent_command_idempotency WHERE session_id = OLD.session_key;
    DELETE FROM user_actions WHERE session_id = OLD.session_key;
END;
