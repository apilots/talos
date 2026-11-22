-- Released v1 replicas are upgraded once by the Talos Parser before v2 use.
ALTER TABLE sync_documents ADD COLUMN replica_schema_version INTEGER NOT NULL DEFAULT 1;

-- Registration changes are owned by the Server directory, not a Sync collector.
DELETE FROM resource_revisions WHERE kind = 'computer';
