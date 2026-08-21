# Error model

[Chinese](ERROR_MODEL_zh.md)

## Ownership

The `talos-error` crate owns the shared error taxonomy:

- `ErrorCode` is the stable machine-readable error identity.
- `ApiError` is the structured public error body.
- `TalosError` carries internal context and source information.
- `TalosResult<T>` is the internal result alias.

Protocol adapters map these objects into HTTP or JSON-RPC envelopes. Stores,
services, SDKs, and UIs must not define competing error taxonomies.

## Error flow

```text
Infrastructure or domain failure
              |
              v
TalosError with context and stable ErrorCode
              |
              v
Service boundary returns ApiError-safe information
              |
              +-- HTTP status + JSON response
              |
              +-- JSON-RPC error response
              |
              `-- Timeline TurnError event
```

## Public contract

A public error contains enough information for deterministic client behavior:

| Field | Purpose |
| --- | --- |
| `code` | Stable `ErrorCode` used by SDK and UI logic |
| `message` | Safe English fallback derived from `ErrorCode` |

Clients branch on `code`, not localized message text. User-facing localization
belongs to the presentation layer. Internal diagnostics, source errors, and
caller locations remain on `TalosError` and are not serialized to clients.

## Protocol mapping

- HTTP selects a status from `ErrorCode` and serializes the shared API error body.
- JSON-RPC keeps JSON-RPC protocol errors distinct from Talos domain errors.
- Streaming failures use a typed Timeline `TurnError` event before terminal completion.
- SDKs expose structured errors rather than transport response bodies.

Transport mapping is centralized in the corresponding gateway adapter. Domain
services do not know HTTP status codes or JSON-RPC numeric protocol codes.

## Safety rules

- Fail fast at the boundary where invalid state becomes known.
- Add operation and resource context before propagating an internal error.
- Preserve the original source for logs and tracing.
- Never expose secrets, filesystem internals, SQL text, or provider credentials.
- Never convert initialization, schema, permission, or transport failures into success.
- Do not use an `InternalError` catch-all when a stable actionable code exists.
- A new `ErrorCode` requires protocol generation and SDK/client compatibility review.

## Testing

Error tests verify the complete public object or envelope, including the stable
code and transport mapping. Bug fixes add a test that reproduces the original
failure path; tests do not depend on message wording unless wording is itself a
public contract.
