-- Replaces legacy ActiveTurn pending payloads with a durable UserAction fact source.
CREATE TABLE user_actions (
    session_id TEXT NOT NULL,
    turn_id TEXT NOT NULL,
    action_id TEXT NOT NULL,
    action_json TEXT NOT NULL,
    response_json TEXT,
    state TEXT NOT NULL,
    version INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (session_id, turn_id, action_id)
);

DROP TRIGGER IF EXISTS sessions_cleanup_after_delete;

CREATE TABLE active_turns_next (
    session_id TEXT PRIMARY KEY,
    turn_id TEXT NOT NULL,
    message_id TEXT,
    status TEXT NOT NULL,
    last_event_seq INTEGER,
    current_action_id TEXT,
    updated_at INTEGER NOT NULL
);

INSERT INTO active_turns_next (
    session_id, turn_id, message_id, status, last_event_seq, current_action_id, updated_at
)
SELECT
    session_id, turn_id, message_id, status, last_event_seq, NULL, updated_at
FROM active_turns
WHERE status <> 'waiting_user_action';

DROP TABLE active_turns;
ALTER TABLE active_turns_next RENAME TO active_turns;

CREATE TRIGGER sessions_cleanup_after_delete
AFTER DELETE ON sessions
BEGIN
    DELETE FROM timeline_events WHERE session_id = OLD.session_key;
    DELETE FROM active_turns WHERE session_id = OLD.session_key;
    DELETE FROM message_idempotency_keys WHERE session_id = OLD.session_key;
    DELETE FROM user_actions WHERE session_id = OLD.session_key;
END;
