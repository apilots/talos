# Client and SDK architecture

[Chinese](CLIENT_ARCHITECTURE_zh.md)

## SDK-first target

Every Talos product client uses a supported SDK. The TypeScript SDK, Rust SDK, and
embedded Rust SDK must expose the same product capability families and shared
domain semantics. A client selects a transport implementation; it must not
select a smaller or different product API. This is the required target contract,
not a substitute for parity tests.

## Client paths

```text
+------------------+  +------------------+  +------------------+  +------------------+
| Web UI           |  | Desktop         |  | TUI / Command    |  | Rust SDK client  |
+--------+---------+  +--------+---------+  +--------+---------+  +--------+---------+
         |                     |                     |                     |
         v                     v                     v                     v
+------------------+  +------------------+  +------------------+  +------------------+
| TS TalosClient   |  | TS AppServer     |  | Rust SDK        |  | Rust SDK        |
| shared types     |  | Client + types   |  | GatewayClient   |  | GatewayClient   |
+--------+---------+  +--------+---------+  +--------+---------+  +--------+---------+
         |                     |                     |                     |
         v                     v                     v                     v
+------------------+  +------------------+  +------------------+  +------------------+
| Browser adapter  |  | Typed preload    |  | EmbeddedServer  |  | Rust AppServer   |
| HTTP + SSE       |  | IPC -> main      |  | Client          |  | Client           |
+--------+---------+  +--------+---------+  +--------+---------+  +--------+---------+
         |                     |                     |                     |
         +---------------------+----------+----------+---------------------+
                                          |
                                          v
+------------------------------------------------------------------------------------+
| HTTP Gateway | main-process JSON-RPC stdio | in-process Gateway | app-server       |
+-----------------------------------------+------------------------------------------+
                                          |
                                          v
+------------------------------------------------------------------------------------+
| Shared TalosService domain results, ErrorCode, and Timeline behavior               |
+------------------------------------------------------------------------------------+
```

## Unified capability surface

SDKs are required to align on product capabilities, typed requests and
responses, errors, and stream behavior:

```text
Authentication and profile
Models and configuration
Sessions, Turns, history, compaction, retry, and interruption
Timeline subscription, replay, recovery, and terminal state
Message queue, steering, dynamic tool results, and user actions
Projects, files, filesystem directories, memory, and workspace documents
Skills, plugins, plugin sources, and lifecycle operations
```

Language-specific naming may be idiomatic, but capability meaning, lifecycle,
error codes, ordering, pagination, optionality, and terminal outcomes stay
aligned. A capability is not complete until the TypeScript SDK facades, Rust SDK,
embedded SDK, and their product transports can all reach it.

## TypeScript SDK

| Package | Responsibility |
| --- | --- |
| `@talos/sdk-core` | Shared capability clients, domain types, errors, and generated protocol bindings |
| `@talos/sdk-browser` | HTTP, multipart upload, authentication header, and SSE transport implementation |
| `@talos/sdk-app-server` | JSON-RPC transport plus Electron main-process and typed preload IPC adapters |

Web uses `TalosClient` through the browser transport. Desktop uses the
TypeScript `AppServerClient` through typed preload IPC and an Electron
main-process transport that owns JSON-RPC stdio. The two facades share generated
domain and protocol types, but they do not currently implement one common
TypeScript client interface. Capability parity is therefore enforced through
API review and parity tests rather than the TypeScript type system alone.

## Rust SDK and embedded SDK

`talos-sdk` owns the Rust `GatewayClient` capability contract, shared public
types, and Rust `AppServerClient`. `AppServerClient` implements the contract over
JSON-RPC stdio.

`talos-sdk-embedded` provides `EmbeddedServerClient`, which implements the same
`GatewayClient` contract over an in-process Gateway. TUI and command mode
therefore use the same compiler-checked SDK operations, responses, errors, and
Timeline behavior as protocol-backed Rust clients while avoiding a child
process or local port.

## Shared type pipeline

```text
Rust domain and current protocol definitions
                  |
                  v
       App Protocol generator
             /          \
            v            v
 JSON Schema        generated/app-protocol.ts
                         |
                         v
             @talos/sdk-core aliases and exports
                         |
                         v
          Browser SDK | App Server SDK | Web | Desktop
```

Generated protocol artifacts come from Rust definitions. Client code must not
hand-edit generated files, copy request or response DTOs, or expose transport
envelopes as domain objects.

## Client ownership

- SDKs own the complete product API, typed domain objects, structured errors,
  stream contracts, and transport-neutral lifecycle semantics.
- Transport adapters own serialization, authentication transport, connection
  lifecycle, upload encoding, and event delivery mechanics.
- Clients own presentation, interaction, optimistic state, and reconnect UX.
- The server owns search, matching, parsing, permission, and durable lifecycle rules.

## Parity requirements

- API changes review Rust SDK, TypeScript SDK, embedded SDK, HTTP, JSON-RPC, Web,
  Desktop, and TUI impact together.
- Current protocol changes regenerate schema and TypeScript bindings with
  `just gen-app-protocol`.
- SDK parity tests compare observable behavior, not transport implementation.
- Embedded execution cannot introduce a private capability unavailable through
  the public SDK contract.
- HTTP and JSON-RPC envelopes may differ, but domain results and `ErrorCode`
  semantics agree.
- User-visible Timeline changes require rendering coverage in every affected client.

The Rust trait currently enforces parity between its app-server and embedded
implementations. TypeScript Browser and App Server facades require explicit
parity review because they are separate APIs. Documentation must not describe a
new capability as complete until those paths and their tests are aligned.
