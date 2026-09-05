-- Rebuilds Session storage around normalized Session-owned facts.
-- Request-scoped context identity and the model duplicated from conf_json are removed.
-- This release has no runtime reader for the legacy execution state. Upgrading
-- with a non-terminal legacy Turn would silently discard recovery ownership, so
-- the offline migration must stop until the previous release settles it.
CREATE TEMP TABLE legacy_active_turn_preflight (
    unsettled_count INTEGER NOT NULL,
    CONSTRAINT active_turns_must_be_settled CHECK (unsettled_count = 0)
);
INSERT INTO legacy_active_turn_preflight (unsettled_count)
SELECT COUNT(*)
FROM active_turns
WHERE status NOT IN ('completed', 'failed');
DROP TABLE legacy_active_turn_preflight;

CREATE TABLE sessions_normalized (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_key TEXT UNIQUE NOT NULL,
    agent_id TEXT NOT NULL CHECK (length(trim(agent_id)) > 0),
    workspace_id TEXT NOT NULL CHECK (length(trim(workspace_id)) > 0),
    project_id TEXT,
    title TEXT NOT NULL DEFAULT 'Untitled Session',
    user_id TEXT NOT NULL CHECK (length(trim(user_id)) > 0),
    source_type TEXT NOT NULL CHECK (
        source_type IN ('http', 'desktop', 'im', 'cli', 'ws', 'system')
    ),
    source_provider TEXT NOT NULL CHECK (
        source_provider IN ('http', 'sdk', 'cli', 'telegram', 'websocket', 'system')
    ),
    channel_id TEXT,
    thread_id TEXT,
    environment_json TEXT NOT NULL CHECK (json_valid(environment_json)),
    conf_json TEXT NOT NULL CHECK (json_valid(conf_json)),
    current_directory TEXT,
    created_at INTEGER NOT NULL,
    last_activity INTEGER NOT NULL,
    message_count INTEGER NOT NULL DEFAULT 0,
    latest_turn_seq INTEGER NOT NULL DEFAULT 0,
    memory_processed_until_event_seq INTEGER NOT NULL DEFAULT 0,
    usage_json TEXT NOT NULL CHECK (json_valid(usage_json)),
    parent_session_id TEXT,
    parent_turn_id TEXT,
    tree_root_session_id TEXT,
    agent_path TEXT
);

INSERT INTO sessions_normalized (
    id, session_key, agent_id, workspace_id, project_id, title, user_id,
    source_type, source_provider, channel_id, thread_id, environment_json,
    conf_json, current_directory, created_at, last_activity, message_count,
    latest_turn_seq, memory_processed_until_event_seq, usage_json,
    parent_session_id, parent_turn_id, tree_root_session_id, agent_path
)
SELECT
    id,
    session_key,
    agent_id,
    workspace_id,
    project_id,
    title,
    user_id,
    json_extract(context_json, '$.source_type'),
    json_extract(context_json, '$.provider'),
    json_extract(context_json, '$.channel_id'),
    json_extract(context_json, '$.thread_id'),
    COALESCE(json_extract(context_json, '$.environment'), 'null'),
    conf_json,
    current_directory,
    created_at,
    last_activity,
    message_count,
    latest_turn_seq,
    0,
    usage_json,
    parent_session_id,
    parent_turn_id,
    tree_root_session_id,
    agent_path
FROM sessions;

DROP TRIGGER IF EXISTS sessions_cleanup_after_delete;
DROP TRIGGER IF EXISTS sessions_agent_execution_cleanup_after_delete;
DROP INDEX IF EXISTS sessions_tree_agent_path_unique;
DROP TABLE sessions;
ALTER TABLE sessions_normalized RENAME TO sessions;

CREATE UNIQUE INDEX sessions_tree_agent_path_unique
    ON sessions(tree_root_session_id, agent_path)
    WHERE NULLIF(tree_root_session_id, '') IS NOT NULL;

-- Pending input is reconstructed from canonical agent events and continuation bindings.
DROP TABLE IF EXISTS user_actions;

-- Canonical agent_events are the only active execution facts. Preserve the
-- incompatible legacy Timeline rows under an explicit archive name so upgrade
-- does not lose history or require a second runtime reader.
ALTER TABLE timeline_events RENAME TO legacy_timeline_events;
DROP TABLE active_turns;

CREATE TRIGGER sessions_cleanup_after_delete
AFTER DELETE ON sessions
BEGIN
    DELETE FROM agent_events WHERE session_id = OLD.session_key;
    DELETE FROM legacy_timeline_events WHERE session_id = OLD.session_key;
    DELETE FROM agent_active_turns WHERE session_id = OLD.session_key;
    DELETE FROM agent_continuation_bindings WHERE session_id = OLD.session_key;
    DELETE FROM agent_command_idempotency WHERE session_id = OLD.session_key;
END;
