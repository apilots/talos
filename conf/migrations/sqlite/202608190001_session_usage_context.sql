-- Upgrades cumulative TokenUsage JSON into the SessionUsage envelope.
UPDATE sessions
SET usage_json = json_object(
    'tokens', json(usage_json),
    'context', NULL
)
WHERE usage_json IS NOT NULL
  AND json_valid(usage_json)
  AND json_type(usage_json) = 'object'
  AND json_type(usage_json, '$.tokens') IS NULL;
