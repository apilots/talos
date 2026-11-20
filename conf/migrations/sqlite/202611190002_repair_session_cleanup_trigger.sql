DROP TRIGGER sessions_cleanup_after_delete;
CREATE TRIGGER sessions_cleanup_after_delete
AFTER DELETE ON sessions
BEGIN
    DELETE FROM agent_events WHERE session_id = OLD.session_key
      AND NOT EXISTS (SELECT 1 FROM session_event_retention r WHERE r.session_id = OLD.session_key AND r.deleted = 1);
    DELETE FROM session_command_revisions WHERE session_id = OLD.session_key
      AND NOT EXISTS (SELECT 1 FROM session_event_retention r WHERE r.session_id = OLD.session_key AND r.deleted = 1);
    DELETE FROM agent_active_turns WHERE session_id = OLD.session_key;
    DELETE FROM agent_continuation_bindings WHERE session_id = OLD.session_key;
    DELETE FROM agent_command_idempotency WHERE session_id = OLD.session_key;
END;
