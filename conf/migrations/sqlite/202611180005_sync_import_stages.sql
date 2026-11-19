CREATE TABLE sync_import_stages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    hub_id TEXT NOT NULL,
    account_id TEXT NOT NULL,
    session_id TEXT NOT NULL,
    version_json TEXT NOT NULL CHECK (json_valid(version_json)),
    base_generation INTEGER NOT NULL,
    base_event_seq INTEGER NOT NULL,
    next_event INTEGER NOT NULL DEFAULT 0,
    complete INTEGER NOT NULL DEFAULT 0 CHECK (complete IN (0, 1)),
    published INTEGER NOT NULL DEFAULT 0 CHECK (published IN (0, 1))
);
CREATE UNIQUE INDEX sync_one_unpublished_session_import
ON sync_import_stages(hub_id, account_id, session_id)
WHERE published = 0;
CREATE INDEX agent_events_import ON agent_events(import_id, event_seq) WHERE import_id <> 0;

ALTER TABLE sync_event_map RENAME TO sync_event_map_before_staging;
CREATE TABLE sync_event_map (
    hub_id TEXT NOT NULL,
    account_id TEXT NOT NULL,
    session_id TEXT NOT NULL,
    generation INTEGER NOT NULL CHECK (generation >= 0),
    stable_event_key TEXT NOT NULL,
    local_event_id TEXT NOT NULL,
    local_event_seq INTEGER NOT NULL,
    import_id INTEGER NOT NULL DEFAULT 0,
    capture_id TEXT NOT NULL DEFAULT '',
    PRIMARY KEY (hub_id, account_id, session_id, generation, stable_event_key, import_id, capture_id),
    UNIQUE (local_event_id, capture_id)
);
INSERT INTO sync_event_map(hub_id, account_id, session_id, generation, stable_event_key, local_event_id, local_event_seq)
SELECT hub_id, account_id, session_id, generation, stable_event_key, local_event_id, local_event_seq
FROM sync_event_map_before_staging;
DROP TABLE sync_event_map_before_staging;
CREATE INDEX sync_event_map_import ON sync_event_map(import_id, local_event_seq) WHERE import_id <> 0;
CREATE INDEX sync_event_map_local_seq ON sync_event_map(local_event_seq);
CREATE INDEX sync_event_map_capture ON sync_event_map(capture_id, local_event_seq) WHERE capture_id <> '';

CREATE VIEW committed_agent_events AS
SELECT e.* FROM agent_events e WHERE e.import_id = 0
OR EXISTS (SELECT 1 FROM sync_import_stages i WHERE i.id = e.import_id AND i.published = 1);

CREATE VIEW visible_sync_event_map AS
SELECT m.* FROM sync_event_map m WHERE m.import_id = 0
OR EXISTS (SELECT 1 FROM sync_import_stages i WHERE i.id = m.import_id AND i.published = 1);

DROP VIEW visible_agent_events;
CREATE VIEW visible_agent_events AS
SELECT e.* FROM sessions s JOIN committed_agent_events e
ON e.session_id = s.session_key AND e.history_generation = s.history_generation
UNION ALL
SELECT e.* FROM committed_agent_events e
WHERE e.history_generation = 0 AND NOT EXISTS (
    SELECT 1 FROM sessions s WHERE s.session_key = e.session_id
) AND NOT EXISTS (
    SELECT 1 FROM session_event_retention r WHERE r.session_id = e.session_id AND r.deleted = 1
);
