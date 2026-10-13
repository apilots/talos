-- Synchronized Computer directory entries carry no registration credentials.
-- Register still compares the real credential hash; NULL never authorizes a Worker.
CREATE TABLE agent_nodes_directory (
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
            AND (endpoint IS NULL OR length(trim(endpoint)) > 0)
            AND ((credential_hash IS NULL AND credential_expires_at IS NULL)
              OR (credential_hash IS NOT NULL AND length(credential_hash) = 32 AND credential_expires_at IS NOT NULL)))
        OR (mode = 'embedded' AND owner_user_id IS NULL AND endpoint IS NULL AND credential_hash IS NULL
            AND credential_expires_at IS NULL AND lease_token_hash IS NULL)),
    CHECK ((lease_token_hash IS NULL AND lease_expires_at IS NULL)
        OR (lease_token_hash IS NOT NULL AND length(lease_token_hash) = 32
            AND credential_hash IS NOT NULL AND credential_expires_at IS NOT NULL
            AND lease_expires_at IS NOT NULL AND lease_expires_at <= credential_expires_at))
);

INSERT INTO agent_nodes_directory SELECT * FROM agent_nodes;
DROP TABLE agent_nodes;
ALTER TABLE agent_nodes_directory RENAME TO agent_nodes;
CREATE INDEX agent_nodes_owner ON agent_nodes(owner_user_id);
