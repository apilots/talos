-- Tables

-- Stores the rebuildable structured projection of canonical memory files.
CREATE TABLE IF NOT EXISTS memory_entries (
    row_id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id TEXT NOT NULL,
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
    UNIQUE(user_id, memory_id)
);

-- Stores the full-text projection used for bounded memory search.
CREATE VIRTUAL TABLE IF NOT EXISTS memory_entries_fts USING fts5(
    title,
    body,
    content='memory_entries',
    content_rowid='row_id',
    tokenize='trigram'
);

-- Stores source fingerprints used by incremental index rebuilds.
CREATE TABLE IF NOT EXISTS memory_sources (
    user_id TEXT NOT NULL,
    source_path TEXT NOT NULL,
    fingerprint TEXT NOT NULL,
    PRIMARY KEY(user_id, source_path)
);

-- Indexes

CREATE INDEX IF NOT EXISTS idx_memory_entries_user_time
    ON memory_entries(user_id, occurred_at DESC, memory_id);
CREATE INDEX IF NOT EXISTS idx_memory_entries_user_source
    ON memory_entries(user_id, source_path);

-- Triggers

-- Keeps the full-text projection synchronized with memory entry inserts.
CREATE TRIGGER IF NOT EXISTS memory_entries_ai AFTER INSERT ON memory_entries BEGIN
    INSERT INTO memory_entries_fts(rowid, title, body)
    VALUES (new.row_id, new.title, new.body);
END;

-- Keeps the full-text projection synchronized with memory entry deletes.
CREATE TRIGGER IF NOT EXISTS memory_entries_ad AFTER DELETE ON memory_entries BEGIN
    INSERT INTO memory_entries_fts(memory_entries_fts, rowid, title, body)
    VALUES ('delete', old.row_id, old.title, old.body);
END;

-- Keeps the full-text projection synchronized with memory entry updates.
CREATE TRIGGER IF NOT EXISTS memory_entries_au AFTER UPDATE ON memory_entries BEGIN
    INSERT INTO memory_entries_fts(memory_entries_fts, rowid, title, body)
    VALUES ('delete', old.row_id, old.title, old.body);
    INSERT INTO memory_entries_fts(rowid, title, body)
    VALUES (new.row_id, new.title, new.body);
END;
