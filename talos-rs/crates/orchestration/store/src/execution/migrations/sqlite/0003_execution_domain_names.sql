ALTER TABLE worker_sessions RENAME TO sessions;
ALTER TABLE worker_admissions RENAME TO turn_admissions;
ALTER TABLE worker_events RENAME TO agent_events;
ALTER TABLE worker_checkpoints RENAME TO turn_checkpoints;
ALTER TABLE worker_supplements RENAME TO session_history_supplements;

DROP INDEX worker_command_identity;
CREATE UNIQUE INDEX turn_admissions_command_identity ON turn_admissions(session_id, generation, input_id);
DROP INDEX worker_events_replay;
CREATE INDEX agent_events_replay ON agent_events(session_id, generation, event_offset);

-- Keep the original domain input while moving recovery authority to local facts.
UPDATE turn_admissions
SET request_json = json_set(
    json_remove(request_json, '$.params.checkpoint', '$.params.continuation'),
    '$.params.intent',
    CASE
        WHEN json_type(request_json, '$.params.continuation') IS NOT NULL
            AND json_type(request_json, '$.params.continuation') <> 'null'
        THEN 'respondUserAction'
        ELSE 'start'
    END
)
WHERE json_extract(request_json, '$.type') = 'run'
    AND json_type(request_json, '$.params.intent') IS NULL;
