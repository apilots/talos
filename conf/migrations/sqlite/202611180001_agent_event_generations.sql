-- Keep one canonical event body and separate its storage lifetime from visibility.
CREATE TEMP TABLE agent_event_cursor AS SELECT seq FROM sqlite_sequence WHERE name = 'agent_events';
DROP TRIGGER sessions_cleanup_after_delete;
ALTER TABLE agent_events RENAME TO agent_events_before_generations;

CREATE TABLE agent_events (
    event_seq INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL,
    turn_id TEXT NOT NULL,
    sequence INTEGER CHECK (sequence > 0),
    occurred_at INTEGER NOT NULL,
    persisted_at INTEGER NOT NULL,
    payload_json TEXT NOT NULL,
    execution_capture_json TEXT,
    automation_origin_json TEXT,
    automation_input_message_id TEXT,
    automation_accounted_tokens INTEGER NOT NULL DEFAULT 0 CHECK (automation_accounted_tokens >= 0),
    automation_loop_decision_json TEXT,
    history_generation INTEGER NOT NULL DEFAULT 0 CHECK (history_generation >= 0),
    import_id INTEGER NOT NULL DEFAULT 0,
    UNIQUE (session_id, history_generation, turn_id, sequence, import_id)
);

INSERT INTO agent_events (
    event_seq, session_id, turn_id, sequence, occurred_at, persisted_at, payload_json,
    execution_capture_json, automation_origin_json, automation_input_message_id,
    automation_accounted_tokens, automation_loop_decision_json, history_generation
)
SELECT e.event_seq, e.session_id, e.turn_id, e.sequence, e.occurred_at, e.persisted_at, e.payload_json,
       e.execution_capture_json, e.automation_origin_json, e.automation_input_message_id,
       e.automation_accounted_tokens, e.automation_loop_decision_json, COALESCE(s.history_generation, 0)
FROM agent_events_before_generations e
LEFT JOIN sessions s ON s.session_key = e.session_id;

DROP TABLE agent_events_before_generations;
UPDATE sqlite_sequence
SET seq = MAX(seq, COALESCE((SELECT seq FROM agent_event_cursor), 0))
WHERE name = 'agent_events';
INSERT INTO sqlite_sequence (name, seq)
SELECT 'agent_events', seq FROM agent_event_cursor
WHERE NOT EXISTS (SELECT 1 FROM sqlite_sequence WHERE name = 'agent_events');
DROP TABLE agent_event_cursor;

CREATE INDEX agent_events_visible_order ON agent_events(session_id, history_generation, event_seq);
CREATE VIEW visible_agent_events AS
SELECT e.* FROM sessions s JOIN agent_events e
ON e.session_id = s.session_key AND e.history_generation = s.history_generation
UNION ALL
SELECT e.* FROM agent_events e
WHERE e.history_generation = 0 AND NOT EXISTS (
    SELECT 1 FROM sessions s WHERE s.session_key = e.session_id
);

CREATE TRIGGER sessions_cleanup_after_delete
AFTER DELETE ON sessions
BEGIN
    DELETE FROM agent_events WHERE session_id = OLD.session_key;
    DELETE FROM legacy_timeline_events WHERE session_id = OLD.session_key;
    DELETE FROM agent_active_turns WHERE session_id = OLD.session_key;
    DELETE FROM agent_continuation_bindings WHERE session_id = OLD.session_key;
    DELETE FROM agent_command_idempotency WHERE session_id = OLD.session_key;
END;
