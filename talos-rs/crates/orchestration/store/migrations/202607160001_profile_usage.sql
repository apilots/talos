CREATE TABLE profile_turn_usage (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id TEXT NOT NULL,
    session_id TEXT NOT NULL,
    turn_id TEXT NOT NULL,
    last_request_id TEXT NOT NULL,
    model_provider TEXT NOT NULL,
    model_id TEXT NOT NULL,
    input_tokens INTEGER NOT NULL,
    output_tokens INTEGER NOT NULL,
    total_tokens INTEGER NOT NULL,
    cache_creation_input_tokens INTEGER,
    cache_read_input_tokens INTEGER,
    usage_available INTEGER NOT NULL,
    finalized INTEGER NOT NULL,
    usage_date TEXT,
    timezone TEXT NOT NULL,
    completed_at_ms INTEGER,
    UNIQUE(session_id, turn_id)
);

CREATE INDEX idx_profile_turn_usage_user_date
    ON profile_turn_usage(user_id, finalized, usage_date);

CREATE INDEX idx_profile_turn_usage_user_model
    ON profile_turn_usage(user_id, finalized, model_provider, model_id);
