# Talos App Protocol

[Chinese](README_zh.md)

`talos-app-protocol` defines the JSON-RPC method registry, notification
registry, protocol schema generation, and TypeScript protocol bindings for the
Talos app-server protocol.

## Generated Artifacts

The Rust protocol types are the source of truth. Do not edit generated schema
or TypeScript files by hand.

Run this command from the repository root whenever app protocol request,
response, notification, or shared wire types change:

```bash
just gen-app-protocol
```

The generator refreshes these repository paths:

- `talos-rs/crates/gateway/app-protocol/schemas/app-protocol.schema.json`
- `talos-app/ts-sdk/packages/core/src/generated/app-protocol.ts`

The generated TypeScript file is consumed by `@talos/sdk-core` and then
re-exported through the browser and desktop-facing SDK packages. Product code
should import protocol request and response types from the SDK packages rather
than duplicating app protocol DTOs.

## Review Checklist

After running the generator:

- Review both generated diffs before committing.
- Confirm Rust serde rename rules and generated TypeScript field names match.
- Run the app protocol schema tests.
- Run the TypeScript SDK typecheck if the generated TypeScript file changed.

Useful checks:

```bash
just test -p talos-app-protocol schema
pnpm --dir talos-app --filter @talos/sdk-core typecheck
```
