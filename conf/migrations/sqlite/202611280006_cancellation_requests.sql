-- Cancellation is a product request, independent of observed Worker lifecycle facts.
ALTER TABLE agent_active_turns ADD COLUMN cancellation_request_id TEXT
    CHECK (cancellation_request_id IS NULL OR length(cancellation_request_id) > 0);

-- Preserve pending requests created before cancellation had its own identity.
UPDATE agent_active_turns
SET cancellation_request_id = lower(hex(randomblob(16))),
    state = CASE (
        SELECT json_extract(payload_json, '$.type')
        FROM agent_events WHERE event_seq = agent_active_turns.last_event_seq
    ) WHEN 'waitingForInput' THEN 'waiting_for_input' ELSE 'running' END,
    updated_at = (SELECT persisted_at FROM agent_events WHERE event_seq = agent_active_turns.last_event_seq)
WHERE state = 'cancelling';
