-- The SQLite index is derived from canonical Workspace Markdown and can be rebuilt.
DROP TRIGGER IF EXISTS memory_entries_ai;
DROP TRIGGER IF EXISTS memory_entries_ad;
DROP TRIGGER IF EXISTS memory_entries_au;
DROP TABLE IF EXISTS memory_entries_fts;
DROP TABLE IF EXISTS memory_sources;
DROP TABLE IF EXISTS memory_entries;

CREATE TABLE memory_entries (
    row_id INTEGER PRIMARY KEY AUTOINCREMENT,
    workspace_id TEXT NOT NULL,
    user_id TEXT NOT NULL,
    workspace_root TEXT NOT NULL,
    memory_id TEXT NOT NULL,
    title TEXT NOT NULL,
    body TEXT NOT NULL,
    kind TEXT NOT NULL,
    importance TEXT NOT NULL,
    occurred_at INTEGER NOT NULL,
    session_id TEXT,
    source_from_seq INTEGER,
    source_until_seq INTEGER,
    source_path TEXT NOT NULL,
    source_date TEXT NOT NULL,
    UNIQUE(workspace_id, user_id, workspace_root, memory_id)
);

CREATE VIRTUAL TABLE memory_entries_fts USING fts5(
    title,
    body,
    content='memory_entries',
    content_rowid='row_id',
    tokenize='trigram'
);

CREATE TABLE memory_sources (
    workspace_id TEXT NOT NULL,
    user_id TEXT NOT NULL,
    workspace_root TEXT NOT NULL,
    source_path TEXT NOT NULL,
    fingerprint TEXT NOT NULL,
    PRIMARY KEY(workspace_id, user_id, workspace_root, source_path)
);

CREATE INDEX idx_memory_entries_scope_time
    ON memory_entries(workspace_id, user_id, workspace_root, occurred_at DESC, memory_id);
CREATE INDEX idx_memory_entries_scope_source
    ON memory_entries(workspace_id, user_id, workspace_root, source_path);

CREATE TRIGGER memory_entries_ai AFTER INSERT ON memory_entries BEGIN
    INSERT INTO memory_entries_fts(rowid, title, body)
    VALUES (new.row_id, new.title, new.body);
END;

CREATE TRIGGER memory_entries_ad AFTER DELETE ON memory_entries BEGIN
    INSERT INTO memory_entries_fts(memory_entries_fts, rowid, title, body)
    VALUES ('delete', old.row_id, old.title, old.body);
END;

CREATE TRIGGER memory_entries_au AFTER UPDATE ON memory_entries BEGIN
    INSERT INTO memory_entries_fts(memory_entries_fts, rowid, title, body)
    VALUES ('delete', old.row_id, old.title, old.body);
    INSERT INTO memory_entries_fts(rowid, title, body)
    VALUES (new.row_id, new.title, new.body);
END;
