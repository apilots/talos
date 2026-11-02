-- Optional private execution facts. Canonical event JSON and sequences are unchanged.
-- Existing history remains unchanged; missing execution details stay NULL.
ALTER TABLE agent_events ADD COLUMN execution_capture_json TEXT;
