-- Provider continuation references belong only to the local execution checkpoint.
ALTER TABLE turn_checkpoints ADD COLUMN waiting_sequence INTEGER CHECK (waiting_sequence > 0);
ALTER TABLE turn_checkpoints ADD COLUMN provider_continuation_json TEXT;

WITH waiting AS (
    SELECT e.session_id, e.generation, e.turn_id, e.sequence,
           json_extract(e.settlement_json, '$.continuation') AS continuation
    FROM agent_events e
    JOIN sessions s ON s.session_id = e.session_id
        AND s.generation = e.generation AND s.active_turn_id = e.turn_id
    WHERE json_extract(e.settlement_json, '$.type') = 'waitingForInput'
      AND json_type(e.settlement_json, '$.continuation') = 'object'
      AND e.event_offset = (
          SELECT MAX(latest.event_offset) FROM agent_events latest
          WHERE latest.session_id = e.session_id AND latest.generation = e.generation
            AND latest.turn_id = e.turn_id AND latest.settlement_json IS NOT NULL
      )
)
UPDATE turn_checkpoints
SET (waiting_sequence, provider_continuation_json) = (
    SELECT sequence, continuation FROM waiting w
    WHERE w.session_id = turn_checkpoints.session_id
      AND w.generation = turn_checkpoints.generation AND w.turn_id = turn_checkpoints.turn_id
)
WHERE EXISTS (
    SELECT 1 FROM waiting w WHERE w.session_id = turn_checkpoints.session_id
      AND w.generation = turn_checkpoints.generation AND w.turn_id = turn_checkpoints.turn_id
);

UPDATE agent_events SET settlement_json = json_remove(settlement_json, '$.continuation')
WHERE json_extract(settlement_json, '$.type') = 'waitingForInput';
