-- Columns

-- Adds the strongly mapped identity fields for Subagent sessions.
ALTER TABLE sessions ADD COLUMN parent_session_id TEXT;
ALTER TABLE sessions ADD COLUMN parent_turn_id TEXT;
ALTER TABLE sessions ADD COLUMN tree_root_session_id TEXT;
ALTER TABLE sessions ADD COLUMN agent_path TEXT;

-- Indexes

-- Prevents duplicate canonical Agent paths within one Session tree.
CREATE UNIQUE INDEX sessions_tree_agent_path_unique
    ON sessions(tree_root_session_id, agent_path)
    WHERE NULLIF(tree_root_session_id, '') IS NOT NULL;

-- Triggers

-- Removes Session-owned runtime records after any Session deletion.
CREATE TRIGGER sessions_cleanup_after_delete
AFTER DELETE ON sessions
BEGIN
    DELETE FROM timeline_events WHERE session_id = OLD.session_key;
    DELETE FROM active_turns WHERE session_id = OLD.session_key;
    DELETE FROM message_idempotency_keys WHERE session_id = OLD.session_key;
END;
