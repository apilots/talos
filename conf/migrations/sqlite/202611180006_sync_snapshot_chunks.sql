ALTER TABLE sync_documents ADD COLUMN snapshot_id TEXT;

CREATE TABLE sync_snapshots (
    id TEXT PRIMARY KEY,
    state TEXT NOT NULL CHECK (state IN ('staged', 'published', 'retired')),
    bytes INTEGER NOT NULL
);
CREATE INDEX sync_snapshots_retired ON sync_snapshots(id) WHERE state = 'retired';

CREATE TABLE sync_snapshot_chunks (
    snapshot_id TEXT NOT NULL REFERENCES sync_snapshots(id),
    ordinal INTEGER NOT NULL,
    contents BLOB NOT NULL,
    PRIMARY KEY (snapshot_id, ordinal)
);
