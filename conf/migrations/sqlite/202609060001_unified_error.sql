-- Migrate canonical failure payloads atomically through the SQLx ledger.
-- Legacy Internal messages contained diagnostics and are not safe public text.
CREATE TEMP TABLE error_contract_validation (valid INTEGER NOT NULL CHECK (valid = 1));
INSERT INTO error_contract_validation
SELECT CASE WHEN
    json_type(payload_json, '$.data.error.message') = 'text'
    AND length(trim(json_extract(payload_json, '$.data.error.message'))) > 0
    AND json_type(payload_json, '$.data.error.retryable') IN ('true', 'false')
    AND json_extract(payload_json, '$.data.error.code') IN (
        'invalidRequest', 'invalidRetryRequest', 'unauthenticated', 'permissionDenied',
        'resourceNotFound', 'sessionNotFound', 'resourceExpired', 'conflict',
        'activeTurnRunning', 'messageQueueFull', 'messageQueueBusy', 'retrySourceCompleted',
        'retrySourceMismatch', 'idempotencyConflict', 'invalidCursor', 'eventCursorExpired',
        'fsDisabled', 'invalidPath', 'pathOutsideRoot', 'symlinkNotSupported', 'pathNotFound',
        'notDirectory', 'invalidName', 'alreadyExists', 'uploadTooLarge', 'unsupportedMediaType',
        'attachmentLimitExceeded', 'unsupportedAttachment', 'fileInputRejected',
        'contextWindowExceeded', 'sessionBudgetExceeded', 'usageLimitExceeded',
        'contextCompactionFailed', 'providerUnavailable', 'toolExecutionFailed', 'timeout',
        'cancelled', 'serviceUnavailable', 'runForcedAbort', 'runCleanupUnconfirmed', 'internalError'
    )
    THEN 1 ELSE 0 END
FROM agent_events WHERE json_extract(payload_json, '$.type') = 'turnFailed';

UPDATE agent_events SET payload_json = json_set(payload_json, '$.data', json_object(
    'error', json_object(
        'code', json_extract(payload_json, '$.data.error.code'),
        'message', CASE
            WHEN json_extract(payload_json, '$.data.error.code') = 'internalError'
                THEN 'The previous model execution failed; legacy technical details were withheld.'
            WHEN length(json_extract(payload_json, '$.data.error.message')) > 4096
                THEN substr(json_extract(payload_json, '$.data.error.message'), 1, 4084) || ' [truncated]'
            ELSE json_extract(payload_json, '$.data.error.message') END,
        'localization', NULL
    ),
    'retryable', json(CASE json_extract(payload_json, '$.data.error.retryable') WHEN 1 THEN 'true' ELSE 'false' END)
)) WHERE json_extract(payload_json, '$.type') = 'turnFailed';
DROP TABLE error_contract_validation;
