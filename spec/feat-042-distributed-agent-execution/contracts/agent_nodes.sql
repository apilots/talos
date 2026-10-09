-- Design attachment only; not a production migration. Times are Unix seconds.
PRAGMA foreign_keys = ON;
CREATE TABLE agent_nodes (
    -- Immutable: CreateNode issues a UUID v4; embedded keeps its reserved identity.
    node_id TEXT PRIMARY KEY NOT NULL,
    owner_user_id TEXT,
    display_name TEXT NOT NULL,
    mode TEXT NOT NULL CHECK (mode IN ('embedded', 'remote')),
    endpoint TEXT,
    status TEXT NOT NULL DEFAULT 'enabled' CHECK (status IN ('enabled', 'disabled')),
    credential_hash BLOB,
    credential_expires_at INTEGER,
    lease_token_hash BLOB,
    lease_expires_at INTEGER,
    last_heartbeat_at INTEGER,
    health TEXT NOT NULL DEFAULT 'unknown' CHECK (health IN ('unknown', 'healthy', 'degraded')),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    CHECK ((mode = 'remote' AND owner_user_id IS NOT NULL AND length(trim(owner_user_id)) > 0
            AND endpoint IS NOT NULL AND length(endpoint) > 0
            AND credential_hash IS NOT NULL AND length(credential_hash) = 32
            AND credential_expires_at IS NOT NULL)
        OR (mode = 'embedded' AND owner_user_id IS NULL AND endpoint IS NULL AND credential_hash IS NULL
            AND credential_expires_at IS NULL AND lease_token_hash IS NULL)),
    CHECK ((lease_token_hash IS NULL AND lease_expires_at IS NULL)
        OR (lease_token_hash IS NOT NULL AND length(lease_token_hash) = 32
            AND lease_expires_at IS NOT NULL AND lease_expires_at <= credential_expires_at))
);
CREATE INDEX agent_nodes_owner ON agent_nodes(owner_user_id);
-- Routing affinity for the existing Session and its node-local Workspace.
ALTER TABLE sessions ADD COLUMN execution_node_id TEXT NOT NULL DEFAULT 'embedded'
    CHECK (length(trim(execution_node_id)) > 0);
