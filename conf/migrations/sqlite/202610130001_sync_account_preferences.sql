-- Account consent belongs to this management host, independent of hub discovery.
CREATE TABLE sync_account_preferences (
    account_id TEXT PRIMARY KEY NOT NULL,
    enabled INTEGER NOT NULL CHECK (enabled IN (0, 1))
);
INSERT INTO sync_account_preferences(account_id, enabled)
SELECT account_id, MIN(enabled) FROM sync_bindings GROUP BY account_id;
ALTER TABLE sync_bindings DROP COLUMN enabled;
