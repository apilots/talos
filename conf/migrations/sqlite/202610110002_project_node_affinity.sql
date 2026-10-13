-- Conflicting historical bindings require an explicit operator decision before migration.
-- This transaction never rewrites Session bindings or guesses between several Workers.
CREATE TEMP TABLE project_node_affinity_preflight (
    conflict_count INTEGER NOT NULL CONSTRAINT project_history_has_multiple_nodes CHECK (conflict_count = 0)
);
INSERT INTO project_node_affinity_preflight
SELECT COUNT(*) FROM (
    SELECT project_id FROM sessions WHERE project_id IS NOT NULL
    GROUP BY project_id HAVING COUNT(DISTINCT execution_node_id) > 1
);
DROP TABLE project_node_affinity_preflight;

ALTER TABLE projects ADD COLUMN execution_node_id TEXT NOT NULL DEFAULT 'embedded'
    CHECK (length(trim(execution_node_id)) > 0);
UPDATE projects SET execution_node_id = COALESCE(
    (SELECT MIN(s.execution_node_id) FROM sessions s WHERE s.project_id = projects.project_id),
    'embedded'
);
CREATE UNIQUE INDEX projects_owner_node_directory ON projects(user_id, execution_node_id, directory);
CREATE INDEX sessions_owner_node_activity ON sessions(user_id, execution_node_id, last_activity DESC);
