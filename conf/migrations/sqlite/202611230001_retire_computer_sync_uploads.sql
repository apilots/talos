-- Computer registration and metadata are owned by the control-plane directory.
-- Retire obsolete upload work without modifying retained checkpoints or ACK cuts.
DELETE FROM sync_resource_capture WHERE kind = 'computer';
DELETE FROM sync_outbox WHERE kind = 'computer';
