CREATE TABLE worker_sessions (
    session_id TEXT PRIMARY KEY,
    binding_json TEXT NOT NULL,
    generation INTEGER NOT NULL DEFAULT 0,
    active_turn_id TEXT,
    retired INTEGER NOT NULL DEFAULT 0,
    memory_processed_until INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE worker_admissions (
    session_id TEXT NOT NULL REFERENCES worker_sessions(session_id),
    generation INTEGER NOT NULL,
    turn_id TEXT NOT NULL,
    input_id TEXT NOT NULL,
    request_json TEXT NOT NULL,
    first_offset INTEGER NOT NULL,
    last_offset INTEGER NOT NULL,
    PRIMARY KEY (session_id, generation, turn_id, input_id)
);
CREATE UNIQUE INDEX worker_command_identity ON worker_admissions(session_id, generation, input_id);
CREATE TABLE worker_events (
    event_offset INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL REFERENCES worker_sessions(session_id),
    generation INTEGER NOT NULL,
    turn_id TEXT NOT NULL,
    sequence INTEGER NOT NULL,
    event_json TEXT NOT NULL,
    settlement_json TEXT,
    UNIQUE (session_id, generation, turn_id, sequence)
);
CREATE INDEX worker_events_replay ON worker_events(session_id, generation, event_offset);
CREATE TABLE worker_checkpoints (
    session_id TEXT NOT NULL,
    generation INTEGER NOT NULL,
    turn_id TEXT NOT NULL,
    checkpoint_json TEXT NOT NULL,
    PRIMARY KEY (session_id, generation, turn_id)
);
CREATE TABLE worker_supplements (
    session_id TEXT NOT NULL,
    generation INTEGER NOT NULL,
    turn_id TEXT NOT NULL,
    supplement_json TEXT NOT NULL,
    PRIMARY KEY (session_id, generation, turn_id)
);
