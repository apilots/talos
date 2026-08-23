ALTER TABLE profile_turn_usage RENAME TO profile_turn_usage_legacy;

DROP INDEX idx_profile_turn_usage_user_date;
DROP INDEX idx_profile_turn_usage_user_model;
DROP INDEX idx_profile_turn_usage_finalized_completed;

CREATE TABLE profile_turn_usage (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id TEXT NOT NULL,
    usage_kind TEXT NOT NULL,
    session_id TEXT,
    turn_id TEXT,
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

INSERT INTO profile_turn_usage (
    id, user_id, usage_kind, session_id, turn_id, last_request_id,
    model_provider, model_id, input_tokens, output_tokens, total_tokens,
    cache_creation_input_tokens, cache_read_input_tokens, usage_available,
    finalized, usage_date, timezone, completed_at_ms
)
SELECT
    id, user_id, 'turn', session_id, turn_id, last_request_id,
    model_provider, model_id, input_tokens, output_tokens, total_tokens,
    cache_creation_input_tokens, cache_read_input_tokens, usage_available,
    finalized, usage_date, timezone, completed_at_ms
FROM profile_turn_usage_legacy;

DROP TABLE profile_turn_usage_legacy;

CREATE INDEX idx_profile_turn_usage_user_date
    ON profile_turn_usage(user_id, finalized, usage_date);

CREATE INDEX idx_profile_turn_usage_user_model
    ON profile_turn_usage(user_id, finalized, model_provider, model_id);

CREATE INDEX idx_profile_turn_usage_finalized_completed
    ON profile_turn_usage(finalized, completed_at_ms);

CREATE UNIQUE INDEX idx_profile_model_call_usage_request
    ON profile_turn_usage(user_id, usage_kind, last_request_id)
    WHERE usage_kind != 'turn';
