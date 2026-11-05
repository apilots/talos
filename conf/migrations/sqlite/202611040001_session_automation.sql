-- Definitions are durable; the existing queue and reservations remain process-local.
CREATE TABLE session_goals (
 id TEXT PRIMARY KEY, session_id TEXT NOT NULL,
 objective TEXT NOT NULL, objective_revision INTEGER NOT NULL CHECK(objective_revision > 0),
 status TEXT NOT NULL CHECK(status IN ('active','paused','complete','cleared')),
 pause_reason TEXT, reason TEXT, token_budget INTEGER CHECK(token_budget > 0),
 tokens_used INTEGER NOT NULL DEFAULT 0 CHECK(tokens_used >= 0), latest_turn_id TEXT,
 completion_evidence TEXT, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL
);
CREATE UNIQUE INDEX session_goals_unfinished ON session_goals(session_id) WHERE status IN ('active','paused');
CREATE INDEX session_goals_session ON session_goals(session_id,created_at,id);
CREATE TABLE session_loops (
 id TEXT PRIMARY KEY, session_id TEXT NOT NULL,
 activation_turn_id TEXT, activation_call_id TEXT, cleared_at INTEGER,
 prompt TEXT NOT NULL, schedule_type TEXT NOT NULL CHECK(schedule_type IN ('fixedInterval','modelDecided')),
 minutes INTEGER, config_revision INTEGER NOT NULL CHECK(config_revision > 0),
 status TEXT NOT NULL CHECK(status IN ('active','stopped')), stop_reason TEXT, reason TEXT,
 next_due_at INTEGER, latest_turn_id TEXT,
 created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL,
 CHECK((schedule_type='fixedInterval' AND minutes IS NOT NULL AND minutes>=1) OR (schedule_type='modelDecided' AND minutes IS NULL))
);
CREATE UNIQUE INDEX session_loops_creation_call ON session_loops(session_id,activation_turn_id,activation_call_id) WHERE activation_call_id IS NOT NULL;
CREATE INDEX session_loops_due ON session_loops(status,next_due_at);
CREATE INDEX session_loops_session ON session_loops(session_id,created_at,id);
ALTER TABLE agent_events ADD COLUMN automation_origin_json TEXT;
ALTER TABLE agent_events ADD COLUMN automation_input_message_id TEXT;
ALTER TABLE agent_events ADD COLUMN automation_accounted_tokens INTEGER NOT NULL DEFAULT 0 CHECK(automation_accounted_tokens>=0);
CREATE TRIGGER sessions_automation_cleanup AFTER DELETE ON sessions BEGIN
 DELETE FROM session_goals WHERE session_id=OLD.session_key;
 DELETE FROM session_loops WHERE session_id=OLD.session_key;
END;
