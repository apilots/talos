-- Run inside the product repair transaction after event reconstruction and
-- after all artifact replacements are durable.
UPDATE agent_active_turns SET last_event_seq = COALESCE((SELECT new_seq FROM history_repair_cursor_0_1_8 WHERE old_seq = last_event_seq), last_event_seq);
UPDATE agent_continuation_bindings SET expected_event_seq = COALESCE((SELECT new_seq FROM history_repair_cursor_0_1_8 WHERE old_seq = expected_event_seq), expected_event_seq);
UPDATE agent_command_idempotency SET committed_event_seq = COALESCE((SELECT new_seq FROM history_repair_cursor_0_1_8 WHERE old_seq = committed_event_seq), committed_event_seq);
UPDATE sessions SET memory_processed_until_event_seq = COALESCE((SELECT new_seq FROM history_repair_cursor_0_1_8 WHERE old_seq = memory_processed_until_event_seq), memory_processed_until_event_seq);
UPDATE agent_command_idempotency SET committed_event_seq = (
    SELECT event_seq FROM agent_events
    WHERE agent_events.session_id = agent_command_idempotency.session_id
      AND agent_events.turn_id = agent_command_idempotency.turn_id AND sequence = 1
) WHERE committed_event_seq IS NULL AND outcome IN ('accepted', 'failed');
DROP TABLE history_repair_cursor_0_1_8;

DROP TRIGGER sessions_cleanup_after_delete;
DROP TABLE legacy_timeline_events;
CREATE TRIGGER sessions_cleanup_after_delete
AFTER DELETE ON sessions
BEGIN
    DELETE FROM agent_events WHERE session_id = OLD.session_key;
    DELETE FROM agent_active_turns WHERE session_id = OLD.session_key;
    DELETE FROM agent_continuation_bindings WHERE session_id = OLD.session_key;
    DELETE FROM agent_command_idempotency WHERE session_id = OLD.session_key;
END;
