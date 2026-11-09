CREATE TABLE managed_workers (
    instance_id TEXT PRIMARY KEY NOT NULL,
    owner_user_id TEXT NOT NULL UNIQUE,
    node_id TEXT NOT NULL UNIQUE REFERENCES agent_nodes(node_id),
    short_id TEXT NOT NULL UNIQUE CHECK(length(short_id) = 8),
    image TEXT NOT NULL,
    revision INTEGER NOT NULL CHECK(revision > 0),
    generation INTEGER NOT NULL CHECK(generation > 0),
    desired TEXT NOT NULL CHECK(desired IN ('running','idle_sleep','manual_stop','deleting')),
    operation TEXT CHECK(operation IN ('create','start','wake','sleep','stop','reset','delete')),
    step TEXT NOT NULL CHECK(step IN ('provision','ensure','start','wait','stop','remove_for_reset','remove_for_delete','remove_home','recover_credential','finalize_delete','stable')),
    home_ready INTEGER NOT NULL CHECK(home_ready IN (0, 1)),
    last_activity_at INTEGER NOT NULL,
    operation_at INTEGER NOT NULL,
    error TEXT CHECK(error IS NULL OR json_valid(error))
);

CREATE TABLE managed_worker_deletions (
    instance_id TEXT PRIMARY KEY NOT NULL,
    owner_user_id TEXT NOT NULL,
    node_id TEXT NOT NULL,
    generation INTEGER NOT NULL
);
