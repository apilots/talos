-- Registration tombstones retain ownership for safe retries and historical references.
-- They contain no credential, lease, or executable directory entry.
CREATE TABLE computer_deletions (
    node_id TEXT PRIMARY KEY NOT NULL,
    owner_user_id TEXT NOT NULL,
    deleted_at INTEGER NOT NULL
);
CREATE TRIGGER prevent_computer_registration_resurrection
BEFORE INSERT ON agent_nodes
WHEN EXISTS (SELECT 1 FROM computer_deletions WHERE node_id = NEW.node_id)
BEGIN
    SELECT RAISE(ABORT, 'Computer registration was deleted');
END;

-- Record the one-time conversion of a host-local alias to its historical UUID.
CREATE TABLE local_computer_retirements (
    node_id TEXT PRIMARY KEY NOT NULL,
    owner_user_id TEXT NOT NULL,
    finalized INTEGER NOT NULL DEFAULT 0 CHECK(finalized IN (0, 1))
);
CREATE TABLE local_computer_retirement_resources (
    node_id TEXT NOT NULL,
    kind TEXT NOT NULL CHECK(kind IN ('session', 'project')),
    resource_id TEXT NOT NULL,
    PRIMARY KEY(node_id, kind, resource_id)
);
