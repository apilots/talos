-- Tables

-- Rebuilds legacy Session storage with the persisted Turn sequence column.
CREATE TABLE sessions_next (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_key TEXT UNIQUE NOT NULL,
    user_id TEXT NOT NULL,
    channel TEXT NOT NULL,
    scope TEXT NOT NULL,
    peer_id TEXT NOT NULL,
    agent_id TEXT NOT NULL,
    account_id TEXT,
    thread_id TEXT,
    created_at INTEGER NOT NULL,
    last_activity INTEGER NOT NULL,
    title TEXT NOT NULL DEFAULT 'Untitled Session',
    context_json TEXT,
    model TEXT,
    message_count INTEGER NOT NULL DEFAULT 0,
    latest_turn_seq INTEGER NOT NULL DEFAULT 0,
    memory_processed_until_seq INTEGER NOT NULL DEFAULT 0,
    usage_json TEXT,
    conf_json TEXT,
    workspace_id TEXT,
    project_id TEXT,
    current_directory TEXT,
    parent_session_id TEXT,
    parent_turn_id TEXT,
    tree_root_session_id TEXT,
    agent_path TEXT
);

INSERT INTO sessions_next (
    id, session_key, user_id, channel, scope, peer_id, agent_id, account_id,
    thread_id, created_at, last_activity, title, context_json, model,
    message_count, latest_turn_seq, memory_processed_until_seq, usage_json,
    conf_json, workspace_id, project_id, current_directory, parent_session_id,
    parent_turn_id, tree_root_session_id, agent_path
)
SELECT
    id, session_key, user_id, channel, scope, peer_id, agent_id, account_id,
    thread_id, created_at, last_activity, title, context_json, model,
    message_count,
    MAX(memory_processed_until_seq, (message_count + 1) / 2),
    memory_processed_until_seq, usage_json, conf_json, workspace_id, project_id,
    current_directory, parent_session_id, parent_turn_id, tree_root_session_id,
    agent_path
FROM sessions;

DROP TRIGGER IF EXISTS sessions_cleanup_after_delete;
DROP INDEX IF EXISTS sessions_tree_agent_path_unique;
DROP TABLE sessions;
ALTER TABLE sessions_next RENAME TO sessions;

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
