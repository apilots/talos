ALTER TABLE sync_documents ADD COLUMN session_projection_json TEXT;
CREATE INDEX sync_documents_missing_session_projection
ON sync_documents(hub_id, account_id, resource_id)
WHERE kind = 'session' AND session_projection_json IS NULL;
