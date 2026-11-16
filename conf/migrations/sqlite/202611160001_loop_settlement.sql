-- Retain the accepted Loop decision with the original admitted Turn. The Loop
-- records which Turn still needs settlement even after its terminal fact commits.
ALTER TABLE agent_events ADD COLUMN automation_loop_decision_json TEXT;
ALTER TABLE session_loops ADD COLUMN pending_turn_id TEXT;

UPDATE session_loops SET pending_turn_id = latest_turn_id
WHERE next_due_at IS NULL AND status = 'active' AND cleared_at IS NULL
  AND EXISTS (
    SELECT 1 FROM agent_active_turns a
    WHERE a.session_id = session_loops.session_id AND a.turn_id = session_loops.latest_turn_id
  );
