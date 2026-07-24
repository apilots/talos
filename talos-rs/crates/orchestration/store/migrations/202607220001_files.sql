CREATE TABLE files (
    file_id TEXT PRIMARY KEY NOT NULL,
    owner_user_id TEXT NOT NULL,
    project_id TEXT,
    storage_id TEXT NOT NULL,
    object_key TEXT NOT NULL,
    original_filename TEXT NOT NULL,
    mime_type TEXT,
    size_bytes INTEGER,
    status TEXT NOT NULL,
    metadata_json TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE(storage_id, object_key),
    CHECK(status IN ('uploading', 'ready', 'deleting')),
    CHECK(
        (status = 'uploading' AND mime_type IS NULL AND size_bytes IS NULL)
        OR
        (status IN ('ready', 'deleting') AND mime_type IS NOT NULL AND size_bytes IS NOT NULL)
    )
);

CREATE INDEX idx_files_owner_created
    ON files(owner_user_id, created_at DESC, file_id DESC);

CREATE INDEX idx_files_owner_project_created
    ON files(owner_user_id, project_id, created_at DESC, file_id DESC);

CREATE INDEX idx_files_status_updated
    ON files(status, updated_at, file_id);

CREATE TABLE file_tags (
    file_id TEXT NOT NULL,
    tag TEXT NOT NULL,
    PRIMARY KEY(file_id, tag),
    FOREIGN KEY(file_id) REFERENCES files(file_id) ON DELETE CASCADE
);

CREATE INDEX idx_file_tags_tag_file
    ON file_tags(tag, file_id);
