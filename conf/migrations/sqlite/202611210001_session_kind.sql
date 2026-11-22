-- Ordinary Sessions keep the standard default.
ALTER TABLE sessions ADD COLUMN session_kind TEXT NOT NULL DEFAULT 'standard'
    CHECK (session_kind IN ('standard', 'talo'));

-- Current registrations and deletion receipts retain managed ownership.
CREATE TEMP TABLE session_kind_talo_nodes AS
SELECT node_id FROM managed_workers
UNION SELECT node_id FROM managed_worker_deletions;

CREATE TEMP TABLE session_kind_talo_sessions AS
SELECT conversation_session_id AS session_id FROM managed_workers
WHERE conversation_session_id IS NOT NULL
UNION
SELECT session_key FROM sessions
WHERE execution_node_id IN (SELECT node_id FROM session_kind_talo_nodes)
UNION
SELECT resource_id FROM resource_revisions
WHERE kind = 'session'
    AND execution_node_id IN (SELECT node_id FROM session_kind_talo_nodes)
UNION
SELECT resource_id FROM sync_documents
WHERE kind = 'session'
    AND json_extract(session_projection_json, '$.snapshot.session.nodeId')
        IN (SELECT node_id FROM session_kind_talo_nodes)
UNION
SELECT resource_id FROM sync_outbox, json_each(sync_outbox.changes_json) AS change
WHERE kind = 'session' AND json_extract(change.value, '$.type') = 'metadata'
    AND (json_extract(change.value, '$.data.before.data.session.nodeId')
            IN (SELECT node_id FROM session_kind_talo_nodes)
        OR json_extract(change.value, '$.data.after.data.session.nodeId')
            IN (SELECT node_id FROM session_kind_talo_nodes));

UPDATE sessions SET session_kind = 'talo'
WHERE session_key IN (SELECT session_id FROM session_kind_talo_sessions);

-- Capture revisions can outlive their Session. Upgrade only cuts containing
-- a stored row; event-only revisions have no row to project.
UPDATE resource_revisions
SET changes_json = json_insert(changes_json, '$.before.fields.session_kind',
    CASE WHEN resource_id IN (SELECT session_id FROM session_kind_talo_sessions) THEN 'talo' ELSE 'standard' END)
WHERE kind = 'session' AND json_type(changes_json, '$.before.fields') = 'object';

UPDATE resource_revisions
SET changes_json = json_insert(changes_json, '$.after.fields.session_kind',
    CASE WHEN resource_id IN (SELECT session_id FROM session_kind_talo_sessions) THEN 'talo' ELSE 'standard' END)
WHERE kind = 'session' AND json_type(changes_json, '$.after.fields') = 'object';

UPDATE sync_documents
SET session_projection_json = json_insert(session_projection_json, '$.snapshot.session.sessionKind',
    CASE WHEN resource_id IN (SELECT session_id FROM session_kind_talo_sessions) THEN 'talo' ELSE 'standard' END)
WHERE kind = 'session' AND json_type(session_projection_json, '$.snapshot.session') = 'object';

-- Pending metadata retains before/after snapshots alongside ordered Turn facts.
UPDATE sync_outbox
SET changes_json = (
    SELECT json_group_array(json(upgraded))
    FROM (
        SELECT CASE
            WHEN json_extract(change.value, '$.type') <> 'metadata' THEN change.value
            WHEN json_type(change.value, '$.data.before.data.session') = 'object'
                THEN json_insert(change.value,
                    '$.data.before.data.session.sessionKind',
                    CASE WHEN resource_id IN (SELECT session_id FROM session_kind_talo_sessions) THEN 'talo' ELSE 'standard' END,
                    '$.data.after.data.session.sessionKind',
                    CASE WHEN resource_id IN (SELECT session_id FROM session_kind_talo_sessions) THEN 'talo' ELSE 'standard' END)
            ELSE json_insert(change.value, '$.data.after.data.session.sessionKind',
                    CASE WHEN resource_id IN (SELECT session_id FROM session_kind_talo_sessions) THEN 'talo' ELSE 'standard' END)
        END AS upgraded
        FROM json_each(sync_outbox.changes_json) AS change
        ORDER BY change.key
    )
)
WHERE kind = 'session' AND EXISTS (
    SELECT 1 FROM json_each(sync_outbox.changes_json) AS change
    WHERE json_extract(change.value, '$.type') = 'metadata'
);

-- Publish known ownership through the existing metadata collector. Retained
-- replicas keep their historical operations; this cut adds the new field.
INSERT INTO resource_revisions(kind, resource_id, owner_user_id, execution_node_id, changes_json)
SELECT 'session', session_key, user_id, execution_node_id,
    json_object('before', json(json_set(cut, '$.fields.session_kind', 'standard')),
                'after', json(cut))
FROM (
    SELECT s.session_key, s.user_id, s.execution_node_id,
        json_object('fields', json_object(
            'session_key', session_key, 'session_kind', session_kind,
            'agent_id', agent_id, 'workspace_id', workspace_id,
            'workspace_user_id', workspace_user_id, 'user_id', user_id,
            'execution_node_id', execution_node_id, 'title', title,
            'source_type', source_type, 'source_provider', source_provider,
            'channel_id', channel_id, 'thread_id', thread_id,
            'environment_json', environment_json, 'conf_json', conf_json,
            'current_directory', current_directory, 'project_id', project_id,
            'created_at', created_at, 'last_activity', last_activity,
            'message_count', message_count, 'usage_json', usage_json,
            'latest_turn_seq', latest_turn_seq,
            'memory_processed_until_event_seq', memory_processed_until_event_seq,
            'parent_session_id', parent_session_id, 'parent_turn_id', parent_turn_id,
            'tree_root_session_id', tree_root_session_id, 'agent_path', agent_path),
            'generation', history_generation, 'active', NULL,
            'event_tail', COALESCE((SELECT MAX(event_seq) FROM committed_agent_events e
                WHERE e.session_id=s.session_key AND e.history_generation=s.history_generation), 0),
            'command_tail', COALESCE((SELECT MAX(revision) FROM session_command_revisions c
                WHERE c.session_id=s.session_key AND c.history_generation=s.history_generation), 0)) AS cut
    FROM sessions s
    WHERE session_kind='talo' AND EXISTS (SELECT 1 FROM sync_documents d
        WHERE d.kind='session' AND d.resource_id=s.session_key AND d.deleted=0)
);

DROP TABLE session_kind_talo_sessions;
DROP TABLE session_kind_talo_nodes;
