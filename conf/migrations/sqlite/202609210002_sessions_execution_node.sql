-- Existing Sessions remain bound to the original in-process Harness.
ALTER TABLE sessions ADD COLUMN execution_node_id TEXT NOT NULL DEFAULT 'embedded'
    CHECK (length(trim(execution_node_id)) > 0);
