# Fix model streams and declared reasoning controls

Status: Implemented. Bug fix, based on origin/main f18feb80.

## Problem

Custom Chat/Messages/Responses share a fixed 60-second total HTTP timeout.
Active reasoning streams can therefore fail with a generic Stream error.
HTTP 400 responses omit useful classification, and binary thinking configuration
currently prevents explicit effort selection.

## Changes

1. Add optional `http_timeouts` to canonical `ProviderConfig`, shared by native
   slots and `custom_providers.<id>`: `connect_secs: 10`, `response_secs: 120`,
   `stream_idle_secs: 90`, `stream_total_secs: 900`. An explicit policy uses these
   defaults for omitted members. Without a policy, native adapters keep their
   established defaults/`with_timeout`; custom adapters use the shared defaults.
   Validate positive values; independent deadlines need no relative ordering.
   All seven adapters reuse `ProviderHttpClients`, preserving provider builder
   options and choosing response versus stream deadlines at dispatch. Activity
   resets only stream idle. Keep outer budgets and cancellation; update YAML.
   Chat, Responses and Messages describe API dialects, independently of transport.
   Existing requests use HTTP/SSE, including Responses. This fix adds no WebSocket
   transport; future WS handshake, connection reuse and per-response deadlines
   belong to that transport, without duplicating API encoders or reusing HTTP
   client lifetime as an individual response deadline.
2. Preserve typed `Timeout` for HTTP/body timeouts. HTTP rejections consume at
   most 8 KiB within a short deadline; recognize allowlisted upstream codes and
   attach only bounded allowlisted identifiers plus provider/model correlation.
   Never retain raw bodies, credentials, URLs or prompts. Malformed SSE remains
   separate. Existing handling boundaries report errors; no automatic Turn replay.
3. Add optional model `reasoning_efforts`. Omitted permits standard effort values,
   independently of thinking switches; an empty list explicitly disables graded
   effort (and retains a configured binary toggle).
   An active nonempty list includes the configured default; the capability
   projection deduplicates values. Explicit static KV retains its existing
   precedence over generated fields; allowed levels govern session selections.
   Backend validates selections; Web/TUI consume the derived allowed list.
   Generated parameters bind explicit levels to a configured thinking switch:
   Off sends false without effort; enabled sends true and the selected effort.
   Existing protocol serialization remains unchanged: no model-name branches or high-to-xhigh map.

## Boundaries and verification

Server/Worker ownership stays unchanged; no Memory/Dream migration.
Configuration API, Agent protobuf, generated schema/SDK and UI save/reload must
preserve the added fields. Existing tool/history/usage settlement is unchanged.
Use controlled HTTP/SSE tests for progress, idle/total deadlines, cancellation,
HTTP rejection classification and exact reasoning request bodies. Native Worker
paths must prove progressing streams outlive the ordinary response deadline and
request/body/idle failures remain typed, across all six adapters and API dialects. Cover backend
validation, configuration/RPC round trips and Web/TUI options. Keep model editing
actions visible while its body scrolls; filter default efforts and expose invalid
saved choices with an explicit correction. Run scoped nextest,
format/lint/type checks, generators and the relevant model E2E scenario. Real
endpoint results are separate from deterministic checks; unknown HTTP 400 causes
must remain explicitly unknown.
