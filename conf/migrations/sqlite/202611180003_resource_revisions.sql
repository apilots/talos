CREATE TABLE resource_revisions (
    revision INTEGER PRIMARY KEY AUTOINCREMENT,
    kind TEXT NOT NULL,
    resource_id TEXT NOT NULL,
    owner_user_id TEXT NOT NULL,
    execution_node_id TEXT NOT NULL,
    changes_json TEXT NOT NULL CHECK (json_valid(changes_json))
);
CREATE INDEX resource_revisions_order ON resource_revisions(kind, resource_id, revision);
CREATE INDEX resource_revisions_owner ON resource_revisions(owner_user_id, kind, resource_id, revision);

CREATE TABLE sync_resource_capture (
    hub_id TEXT NOT NULL,
    account_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    resource_id TEXT NOT NULL,
    revision INTEGER NOT NULL,
    outbox_sequence INTEGER NOT NULL,
    exported_version_json TEXT,
    PRIMARY KEY (hub_id, account_id, kind, resource_id, revision)
);
CREATE INDEX sync_resource_capture_outbox
ON sync_resource_capture(hub_id, account_id, kind, resource_id, outbox_sequence);

CREATE TABLE session_event_retention (
    session_id TEXT PRIMARY KEY,
    through_generation INTEGER NOT NULL,
    deleted INTEGER NOT NULL CHECK (deleted IN (0, 1)),
    reclaim INTEGER NOT NULL DEFAULT 1 CHECK (reclaim IN (0, 1))
);
CREATE INDEX session_history_pending_reclaim ON session_event_retention(session_id) WHERE reclaim = 1;

CREATE TRIGGER sessions_skip_retained_generation
AFTER INSERT ON sessions
WHEN EXISTS (SELECT 1 FROM session_event_retention WHERE session_id = NEW.session_key AND deleted = 1)
BEGIN
    UPDATE sessions
    SET history_generation = MAX(NEW.history_generation, (
        SELECT through_generation + 1 FROM session_event_retention WHERE session_id = NEW.session_key
    ))
    WHERE session_key = NEW.session_key;
END;

DROP VIEW visible_agent_events;
CREATE VIEW visible_agent_events AS
SELECT e.* FROM sessions s JOIN agent_events e
ON e.session_id = s.session_key AND e.history_generation = s.history_generation
UNION ALL
SELECT e.* FROM agent_events e
WHERE e.history_generation = 0 AND NOT EXISTS (
    SELECT 1 FROM sessions s WHERE s.session_key = e.session_id
) AND NOT EXISTS (
    SELECT 1 FROM session_event_retention r WHERE r.session_id = e.session_id AND r.deleted = 1
);

DROP TRIGGER sessions_cleanup_after_delete;
CREATE TRIGGER sessions_cleanup_after_delete
AFTER DELETE ON sessions
BEGIN
    DELETE FROM agent_events WHERE session_id = OLD.session_key
      AND NOT EXISTS (SELECT 1 FROM session_event_retention r WHERE r.session_id = OLD.session_key AND r.deleted = 1);
    DELETE FROM session_command_revisions WHERE session_id = OLD.session_key
      AND NOT EXISTS (SELECT 1 FROM session_event_retention r WHERE r.session_id = OLD.session_key AND r.deleted = 1);
    DELETE FROM legacy_timeline_events WHERE session_id = OLD.session_key;
    DELETE FROM agent_active_turns WHERE session_id = OLD.session_key;
    DELETE FROM agent_continuation_bindings WHERE session_id = OLD.session_key;
    DELETE FROM agent_command_idempotency WHERE session_id = OLD.session_key;
END;
