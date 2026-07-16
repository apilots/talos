CREATE INDEX idx_profile_turn_usage_finalized_completed
    ON profile_turn_usage(finalized, completed_at_ms);
