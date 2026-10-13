-- Management database synchronization only; Workers own no product database.
CREATE TABLE sync_identity (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    peer_id TEXT NOT NULL UNIQUE,
    loro_writer_id TEXT NOT NULL UNIQUE
);
CREATE TABLE sync_bindings (
    hub_id TEXT NOT NULL,
    account_id TEXT NOT NULL,
    source_user_id TEXT NOT NULL,
    enabled INTEGER NOT NULL CHECK (enabled IN (0, 1)),
    computers_json TEXT NOT NULL,
    download_cursor TEXT,
    PRIMARY KEY (hub_id, account_id, source_user_id),
    UNIQUE (hub_id, source_user_id)
);
CREATE TABLE sync_documents (
    hub_id TEXT NOT NULL,
    account_id TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('computer', 'project', 'session', 'file')),
    resource_id TEXT NOT NULL,
    computer_id TEXT,
    generation INTEGER NOT NULL CHECK (generation >= 0),
    deleted INTEGER NOT NULL CHECK (deleted IN (0, 1)),
    snapshot_blob BLOB NOT NULL,
    version_vector BLOB NOT NULL,
    acknowledged_vector BLOB,
    acknowledged_generation INTEGER NOT NULL DEFAULT 0 CHECK (acknowledged_generation >= 0),
    PRIMARY KEY (hub_id, account_id, kind, resource_id)
);
CREATE TABLE sync_outbox (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    hub_id TEXT NOT NULL,
    account_id TEXT NOT NULL,
    source_user_id TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('computer', 'project', 'session', 'file')),
    resource_id TEXT NOT NULL,
    changes_json TEXT NOT NULL,
    exported_version_json TEXT,
    created_at INTEGER NOT NULL,
    FOREIGN KEY (hub_id, account_id, source_user_id)
        REFERENCES sync_bindings(hub_id, account_id, source_user_id)
);
CREATE INDEX sync_outbox_resource ON sync_outbox(hub_id, account_id, kind, resource_id, seq);
CREATE TABLE sync_event_map (
    hub_id TEXT NOT NULL,
    account_id TEXT NOT NULL,
    session_id TEXT NOT NULL,
    generation INTEGER NOT NULL CHECK (generation >= 0),
    stable_event_key TEXT NOT NULL,
    local_event_id TEXT NOT NULL,
    local_event_seq INTEGER NOT NULL,
    PRIMARY KEY (hub_id, account_id, session_id, generation, stable_event_key),
    UNIQUE (local_event_id)
);
