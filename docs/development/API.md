# Public API conventions

Use when changing public DTOs, wire contracts, or integration behavior. Apply the wire rules to the current App Server protocol; other public APIs still require the compatibility review below.

## Domain objects and extensions

- Name types for domain concepts rather than vague `Item`, `Details`, or `Content` containers. Shared fields belong on the main object and variant-specific fields on typed variants.
- Public fields need reachable management paths; mark intentionally incomplete fields reserved or experimental.

## App Server and JSON-RPC

- Add API surface only to the current major protocol version. Export its request, response, and notification types into the corresponding TypeScript/schema namespace.
- Name DTOs by their owning boundary: Service/Gateway/SDK use `*Request` / `*Response`; JSON-RPC uses `*Params` / `*Response` / `*Notification`; Coordinator, AgentHarness, AgentProvider, and AgentSession SPI use `*Params` / `*Result`. Do not duplicate the same semantic object under multiple suffixes; rename it when ownership changes instead of retaining alias pairs.
- Method names use singular `<resource>/<method>`, such as `thread/read` or `app/list`.
- Wire fields and enum strings use `camelCase`. A configuration API may mirror `snake_case` config keys with an explicit explanation. Keep Rust serde and TypeScript renames and tagged-union tags identical and explicit.
- Use `String` IDs at API boundaries and parse internally. Timestamps are integer Unix seconds (`i64`) with Rust field names ending in `*_at`, such as `created_at`.
- Current-version optional payload fields must not use `skip_serializing_if = "Option::is_none"`. A client request with empty params may explicitly use `Option<()>`.
- Define optional versus nullable semantics for each optional client request field. Use `Option<Vec<T>>` or `Option<HashMap<K, V>>` for optional collections; do not conflate omitted and empty values through `serde(default)`.
- For request booleans whose omission means false, prefer `#[serde(default, skip_serializing_if = "std::ops::Not::not")]` over `Option<bool>`.
- Mark experimental APIs explicitly; field-level experiments must be inspectable by schema/runtime checks.
- New list methods default to cursor pagination: request `cursor` and `limit`, response `data` and `next_cursor`.

## Compatibility review

- When public objects or behavior change, inspect affected app-server, JSON-RPC, HTTP, SDK, CLI, configuration, session recovery, raw events, frontend/TUI events, and plugin integrations. Widen the review for shared-layer changes; experimental APIs with external callers still require compatibility review.
- Explain compatibility effects of removed/renamed fields, defaults, error codes, notification order, and state transitions. Update affected docs, README files, schemas, and examples with API behavior changes.

Run the owning generators and update build inputs as described in [Generated artifacts](DEVELOPMENT.md#generated-artifacts).
