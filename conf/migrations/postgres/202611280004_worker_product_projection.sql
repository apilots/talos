-- Product-only source positions and replay-safe settlement work, never a delivery outbox.
CREATE TABLE worker_projection_cursors (
    session_id TEXT PRIMARY KEY REFERENCES sessions(session_key) ON DELETE CASCADE,
    execution_node_id TEXT NOT NULL,
    history_generation BIGINT NOT NULL,
    worker_offset BIGINT NOT NULL
);
CREATE TABLE worker_event_projection (
    session_id TEXT NOT NULL REFERENCES sessions(session_key) ON DELETE CASCADE,
    history_generation BIGINT NOT NULL,
    worker_offset BIGINT NOT NULL,
    product_event_seq BIGINT NOT NULL,
    event_fingerprint TEXT NOT NULL,
    settlement_json TEXT,
    command_json TEXT,
    execution_model_json TEXT,
    execution_purpose TEXT NOT NULL,
    settlement_applied_at BIGINT,
    PRIMARY KEY (session_id, history_generation, worker_offset)
);
CREATE INDEX worker_pending_settlements ON worker_event_projection(session_id, settlement_applied_at) WHERE settlement_json IS NOT NULL;
