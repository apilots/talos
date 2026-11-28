CREATE TABLE session_dream_cursors (
    session_id TEXT PRIMARY KEY REFERENCES sessions(session_key) ON DELETE CASCADE,
    generation INTEGER NOT NULL,
    formation_offset INTEGER NOT NULL,
    covered_until_source_seq INTEGER NOT NULL DEFAULT 0
);
