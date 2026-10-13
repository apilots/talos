-- The business actor and the immutable Worker filesystem namespace differ after import.
ALTER TABLE sessions ADD COLUMN workspace_user_id TEXT;
UPDATE sessions SET workspace_user_id = user_id;
-- File bodies remain exclusively in their source FileService. Remote metadata
-- must not fabricate a local OpenDAL storage ID or object key.
CREATE TABLE files_sync_locations (
    file_id TEXT PRIMARY KEY NOT NULL, owner_user_id TEXT NOT NULL, project_id TEXT,
    storage_id TEXT, object_key TEXT, storage_peer_id TEXT,
    original_filename TEXT NOT NULL, mime_type TEXT, size_bytes INTEGER,
    status TEXT NOT NULL CHECK(status IN ('uploading', 'ready', 'deleting')),
    metadata_json TEXT NOT NULL, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL,
    UNIQUE(storage_id, object_key),
    CHECK((storage_peer_id IS NULL AND storage_id IS NOT NULL AND object_key IS NOT NULL)
       OR (length(storage_peer_id) > 0 AND storage_id IS NULL AND object_key IS NULL AND status = 'ready')),
    CHECK((status = 'uploading' AND mime_type IS NULL AND size_bytes IS NULL)
       OR (status IN ('ready', 'deleting') AND mime_type IS NOT NULL AND size_bytes IS NOT NULL))
);
INSERT INTO files_sync_locations(file_id, owner_user_id, project_id, storage_id, object_key, original_filename, mime_type, size_bytes, status, metadata_json, created_at, updated_at)
SELECT file_id, owner_user_id, project_id, storage_id, object_key, original_filename, mime_type, size_bytes, status, metadata_json, created_at, updated_at FROM files;
CREATE TEMP TABLE file_tags_preserved AS SELECT * FROM file_tags;
DROP TABLE file_tags;
DROP TABLE files;
ALTER TABLE files_sync_locations RENAME TO files;
CREATE INDEX idx_files_owner_created ON files(owner_user_id, created_at DESC, file_id DESC);
CREATE INDEX idx_files_owner_project_created ON files(owner_user_id, project_id, created_at DESC, file_id DESC);
CREATE INDEX idx_files_status_updated ON files(status, updated_at, file_id);
CREATE TABLE file_tags(file_id TEXT NOT NULL, tag TEXT NOT NULL, PRIMARY KEY(file_id, tag), FOREIGN KEY(file_id) REFERENCES files(file_id) ON DELETE CASCADE);
INSERT INTO file_tags SELECT * FROM file_tags_preserved;
DROP TABLE file_tags_preserved;
CREATE INDEX idx_file_tags_tag_file ON file_tags(tag, file_id);
