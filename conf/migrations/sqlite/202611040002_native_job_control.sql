-- Pause only the actual execution instance's existing native callbacks.
-- Callback definitions, cron configuration and execution results stay outside this table.
CREATE TABLE automation_native_job_controls (
    execution_instance_id TEXT NOT NULL CHECK(length(trim(execution_instance_id)) > 0),
    job_id TEXT NOT NULL CHECK(job_id IN ('profile-usage-retention', 'dream-memory-maintenance')),
    paused INTEGER NOT NULL CHECK(paused IN (0, 1)),
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (execution_instance_id, job_id)
);
