-- Product notices share the Timeline cursor but never consume Agent sequence numbers.
CREATE TEMP TABLE notice_migration_cursor AS SELECT seq FROM sqlite_sequence WHERE name = 'agent_events';
DROP TRIGGER sessions_cleanup_after_delete;
ALTER TABLE agent_events RENAME TO agent_events_before_notices;
CREATE TABLE agent_events (
    event_seq INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL,
    turn_id TEXT NOT NULL,
    sequence INTEGER CHECK (sequence > 0),
    occurred_at INTEGER NOT NULL,
    persisted_at INTEGER NOT NULL,
    payload_json TEXT NOT NULL,
    UNIQUE (session_id, turn_id, sequence)
);
INSERT INTO agent_events SELECT * FROM agent_events_before_notices;
DROP TABLE agent_events_before_notices;
UPDATE sqlite_sequence SET seq = max(seq, coalesce((SELECT seq FROM notice_migration_cursor), 0)) WHERE name = 'agent_events';
INSERT INTO sqlite_sequence (name, seq) SELECT 'agent_events', seq FROM notice_migration_cursor WHERE NOT EXISTS (SELECT 1 FROM sqlite_sequence WHERE name = 'agent_events');
DROP TABLE notice_migration_cursor;
CREATE INDEX idx_agent_events_session_seq ON agent_events(session_id, event_seq);
CREATE INDEX idx_agent_events_turn ON agent_events(session_id, turn_id, sequence);
CREATE TRIGGER sessions_cleanup_after_delete
AFTER DELETE ON sessions
BEGIN
    DELETE FROM agent_events WHERE session_id = OLD.session_key;
    DELETE FROM legacy_timeline_events WHERE session_id = OLD.session_key;
    DELETE FROM agent_active_turns WHERE session_id = OLD.session_key;
    DELETE FROM agent_continuation_bindings WHERE session_id = OLD.session_key;
    DELETE FROM agent_command_idempotency WHERE session_id = OLD.session_key;
END;

-- Upgrade existing informational SystemNotice blocks without guessing their text.
CREATE TEMP TABLE notice_migration_validation (valid INTEGER NOT NULL CHECK (valid = 1));
INSERT INTO notice_migration_validation
SELECT CASE WHEN json_type(payload_json, '$.data.block.content.data.message') = 'text' THEN 1 ELSE 0 END
FROM agent_events WHERE json_extract(payload_json, '$.type') IN ('messageBlockStarted', 'messageBlockCompleted') AND json_extract(payload_json, '$.data.block.content.type') = 'systemNotice';
INSERT INTO notice_migration_validation
SELECT CASE WHEN json_type(block.value, '$.content.data.message') = 'text' THEN 1 ELSE 0 END
FROM agent_events, json_each(payload_json, CASE json_extract(payload_json, '$.type') WHEN 'messageStarted' THEN '$.data.message.blocks' ELSE '$.data.blocks' END) AS block
WHERE json_extract(payload_json, '$.type') IN ('messageStarted', 'messageCompleted') AND json_extract(block.value, '$.content.type') = 'systemNotice';
DROP TABLE notice_migration_validation;
UPDATE agent_events SET payload_json = json_remove(json_set(payload_json, '$.data.block.content.data.content', json_object('type', 'text', 'data', json_extract(payload_json, '$.data.block.content.data.message'))), '$.data.block.content.data.message') WHERE json_extract(payload_json, '$.type') = 'messageBlockStarted' AND json_extract(payload_json, '$.data.block.content.type') = 'systemNotice';
UPDATE agent_events SET payload_json = json_remove(json_set(payload_json, '$.data.block.content.data.content', json_object('type', 'text', 'data', json_extract(payload_json, '$.data.block.content.data.message'))), '$.data.block.content.data.message') WHERE json_extract(payload_json, '$.type') = 'messageBlockCompleted' AND json_extract(payload_json, '$.data.block.content.type') = 'systemNotice';
UPDATE agent_events SET payload_json = json_set(payload_json, '$.data.message.blocks', json((
    SELECT json_group_array(json(CASE WHEN json_extract(value, '$.content.type') = 'systemNotice'
        THEN json_remove(json_set(value, '$.content.data.content', json_object('type', 'text', 'data', json_extract(value, '$.content.data.message'))), '$.content.data.message')
        ELSE value END)) FROM json_each(payload_json, '$.data.message.blocks')
))) WHERE json_extract(payload_json, '$.type') = 'messageStarted';
UPDATE agent_events SET payload_json = json_set(payload_json, '$.data.blocks', json((
    SELECT json_group_array(json(CASE WHEN json_extract(value, '$.content.type') = 'systemNotice'
        THEN json_remove(json_set(value, '$.content.data.content', json_object('type', 'text', 'data', json_extract(value, '$.content.data.message'))), '$.content.data.message')
        ELSE value END)) FROM json_each(payload_json, '$.data.blocks')
))) WHERE json_extract(payload_json, '$.type') = 'messageCompleted';
