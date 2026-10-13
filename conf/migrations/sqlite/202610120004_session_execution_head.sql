-- A Session reset remains causal even when the optional sync transport is off.
ALTER TABLE sessions ADD COLUMN history_generation INTEGER NOT NULL DEFAULT 0 CHECK(history_generation >= 0);
UPDATE sessions SET history_generation = COALESCE((SELECT MAX(generation) FROM sync_documents WHERE kind = 'session' AND resource_id = sessions.session_key), 0);
