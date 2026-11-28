CREATE TABLE dream_formations (
    formation_offset INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL REFERENCES sessions(session_id),
    generation INTEGER NOT NULL,
    formation_json TEXT NOT NULL
);
CREATE INDEX dream_formations_replay ON dream_formations(session_id, generation, formation_offset);
