-- Existing sessions remain local read-only history after the execution ownership cutover.
ALTER TABLE sessions ADD COLUMN execution_architecture TEXT NOT NULL DEFAULT 'legacy_read_only' CHECK (execution_architecture IN ('legacy_read_only', 'replica_read_only', 'worker'));
