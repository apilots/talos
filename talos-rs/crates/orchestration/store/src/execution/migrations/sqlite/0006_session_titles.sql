CREATE TABLE session_title_results (
    session_id TEXT NOT NULL REFERENCES sessions(session_id),
    generation INTEGER NOT NULL,
    turn_id TEXT NOT NULL,
    result_json TEXT NOT NULL,
    PRIMARY KEY(session_id, generation, turn_id)
);
