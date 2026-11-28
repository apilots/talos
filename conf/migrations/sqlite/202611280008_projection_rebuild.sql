-- A bounded rebuild stages source facts before atomically replacing derived rows.
-- Source mappings also retain product settlement receipts and must survive rebuilding.
CREATE TABLE worker_projection_rebuilds (
    session_id TEXT PRIMARY KEY REFERENCES sessions(session_key) ON DELETE CASCADE,
    request_id TEXT NOT NULL UNIQUE,
    execution_node_id TEXT NOT NULL,
    history_generation BIGINT NOT NULL,
    worker_offset BIGINT NOT NULL DEFAULT 0,
    complete INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE worker_projection_rebuild_events (
    request_id TEXT NOT NULL REFERENCES worker_projection_rebuilds(request_id) ON DELETE CASCADE,
    worker_offset BIGINT NOT NULL,
    fact_json TEXT NOT NULL,
    PRIMARY KEY (request_id, worker_offset)
);

-- Keep immutable Sync timestamps even when the corresponding derived row is lost.
ALTER TABLE worker_event_projection ADD COLUMN product_persisted_at BIGINT;
UPDATE worker_event_projection
SET product_persisted_at = (SELECT e.persisted_at FROM agent_events e WHERE e.event_seq=worker_event_projection.product_event_seq);
