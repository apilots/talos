-- Canonical Agent execution facts used by the standardized Agent API.
-- Session normalization makes these the sole execution facts in this release.
CREATE TABLE agent_events (
    event_seq INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL,
    turn_id TEXT NOT NULL,
    sequence INTEGER NOT NULL CHECK (sequence > 0),
    occurred_at INTEGER NOT NULL,
    persisted_at INTEGER NOT NULL,
    payload_json TEXT NOT NULL,
    UNIQUE (session_id, turn_id, sequence)
);

CREATE INDEX idx_agent_events_session_seq
    ON agent_events(session_id, event_seq);
CREATE INDEX idx_agent_events_turn
    ON agent_events(session_id, turn_id, sequence);

CREATE TABLE agent_active_turns (
    session_id TEXT PRIMARY KEY,
    turn_id TEXT NOT NULL,
    initial_message_id TEXT,
    state TEXT NOT NULL CHECK (state IN ('running', 'waiting_for_input', 'cancelling')),
    last_event_seq INTEGER NOT NULL CHECK (last_event_seq > 0),
    updated_at INTEGER NOT NULL
);

CREATE TABLE agent_continuation_bindings (
    session_id TEXT NOT NULL,
    turn_id TEXT NOT NULL,
    action_id TEXT NOT NULL,
    provider_ref_json TEXT NOT NULL,
    expected_event_seq INTEGER NOT NULL CHECK (expected_event_seq > 0),
    status TEXT NOT NULL CHECK (status IN ('waiting', 'claimed')),
    claim_id TEXT,
    PRIMARY KEY (session_id, turn_id),
    CHECK (
        (status = 'waiting' AND claim_id IS NULL)
        OR (status = 'claimed' AND length(claim_id) > 0)
    )
);

CREATE TRIGGER sessions_agent_execution_cleanup_after_delete
AFTER DELETE ON sessions
BEGIN
    DELETE FROM agent_events WHERE session_id = OLD.session_key;
    DELETE FROM agent_active_turns WHERE session_id = OLD.session_key;
    DELETE FROM agent_continuation_bindings WHERE session_id = OLD.session_key;
END;
