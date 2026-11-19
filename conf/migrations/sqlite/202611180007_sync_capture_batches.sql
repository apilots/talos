CREATE TABLE sync_capture_batches (
    id TEXT PRIMARY KEY,
    state TEXT NOT NULL CHECK (state IN ('staged', 'published', 'retired'))
);
CREATE INDEX sync_capture_batches_retired ON sync_capture_batches(id) WHERE state = 'retired';
ALTER TABLE sync_outbox ADD COLUMN capture_id TEXT NOT NULL DEFAULT '';
CREATE INDEX sync_outbox_capture ON sync_outbox(capture_id, seq) WHERE capture_id <> '';

CREATE VIEW visible_sync_outbox AS
SELECT o.* FROM sync_outbox o WHERE o.capture_id = ''
OR EXISTS (SELECT 1 FROM sync_capture_batches c WHERE c.id = o.capture_id AND c.state = 'published');

DROP VIEW visible_sync_event_map;
CREATE VIEW visible_sync_event_map AS
SELECT m.* FROM sync_event_map m
WHERE (m.import_id = 0 OR EXISTS (
    SELECT 1 FROM sync_import_stages i WHERE i.id = m.import_id AND i.published = 1
)) AND (m.capture_id = '' OR EXISTS (
    SELECT 1 FROM sync_capture_batches c WHERE c.id = m.capture_id AND c.state = 'published'
));
