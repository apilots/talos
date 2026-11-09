ALTER TABLE managed_workers ADD COLUMN conversation_session_id TEXT;

UPDATE managed_workers
SET conversation_session_id = 'sess_' || lower(hex(randomblob(16)));

CREATE UNIQUE INDEX managed_workers_conversation_session_idx
ON managed_workers(conversation_session_id);
