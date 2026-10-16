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
| Web UI           |  | Desktop          |  | TUI / Command    |  | Rust SDK client  |
+--------+---------+  +--------+---------+  +--------+---------+  +--------+---------+
         |                     |                     |                     |
         v                     v                     v                     v
+------------------+  +------------------+  +------------------+  +------------------+
| TS TalosClient   |  | TS product SDK   |  | Rust SDK         |  | Rust SDK         |
| shared types     |  | Client + types   |  | GatewayClient    |  | GatewayClient    |
+--------+---------+  +--------+---------+  +--------+---------+  +--------+---------+
         |                     |                     |                     |
         v                     v                     v                     v
+------------------+  +------------------+  +------------------+  +------------------+
| Browser adapter  |  | HTTP/SSE or IPC  |  | EmbeddedServer   |  | Rust AppServer   |
| HTTP + SSE       |  | personal Worker  |  | Client           |  | Client           |
+--------+---------+  +--------+---------+  +--------+---------+  +--------+---------+
         |                     |                     |                     |
         +---------------------+-----------+---------+---------------------+
                                           |
                                           v
+------------------------------------------------------------------------------------+
| HTTP Gateway | main JSON-RPC stdio | in-process Gateway | app-server               |
+------------------------------------------+-----------------------------------------+
                                           |
                                           v
+------------------------------------------------------------------------------------+
| TalosService: shared domain responses, ApiError and durable state                  |
+------------------------------------------+-----------------------------------------+
                                           |
                                           v
+------------------------------------------------------------------------------------+
| Store commit -> Timeline { Agent | SystemNotice } -> subscribers / History         |
+------------------------------------------+-----------------------------------------+
```

## Unified capability surface

SDKs are required to align on product capabilities, typed requests and
responses, errors, and stream behavior:

```text
Authentication and profile
Computer directory, owner management and availability
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

Web uses `TalosClient` over HTTP/SSE to its selected management service. Desktop
uses `AppServerClient` through typed preload IPC and JSON-RPC stdio. Its bundled
app-server retains Service, Store, Coordinator and one `TalosWorker`. Verified
login enrolls or restores this same Worker's personal UUID through the central
Server. Registration does not replace the management runtime or its data.
Rust stdio and embedded clients initialize the existing host registration lifecycle
after verified login or cached-session restoration, including when Sync is disabled.
Remote dispatch stays inside `agent-client`; credentials never reach the renderer.
The production UI requires sign-in. Anonymous development keeps local execution
without registering a fictitious cloud node. Sync opt-out remains independent.
Shared generated types and parity tests keep the browser and Desktop facades aligned.

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

```text
Request -> transport status -> ApiResponse unwrap -> domain response / ApiError
                                                         |
                                              ACK != Turn completion

Session subscription -> replay to high water -> live delivery across Turns
         |                         |
         +-- Agent facts ----------+--> bounded live execution projection
         `-- SystemNotice ---------+--> stable-ID notice blocks
                                            |
History page -------------------------------+--> reconcile, then render
```

The request and subscription lanes can race. Match canonical message identity
and content before retiring optimistic input; an empty MessageStarted is not
proof the user's input has been rendered. Queue revision only invalidates queue
state, not a second execution-state owner. Start/retry finish their request at
ACK; stopping execution uses the actual interrupt API.

The Web/Desktop transcript drains every history page on entry before subscribing
to unfinished execution. `lastStableEventId` is the opaque exclusive event cursor
of the last completed Turn returned by a history page; use the final page's value
after pagination. It is not the Session's current event high water, which can
include an unfinished Turn. The history pagination cursor is not an event cursor.
This response field requires coordinated backend and generated-client deployment;
HTTP and JSON-RPC return the same history contract. Rust SDK/TUI readers may ignore it.

Web/Desktop keeps the consumed event cursor while rendering an unfinished Turn,
then reconciles its terminal output with history without replaying completed
content. Idle Sessions query active-Turn and queue state rather than subscribe to
execution events. The active-Turn response includes `latestEventId`, the latest
durable Session event (including failures and product notices). An idle client
refreshes history when that snapshot changes, even if a remote Turn starts and
finishes between polls. This cursor is a change signal, not a replacement for
`lastStableEventId` when subscribing. Backend and generated clients must upgrade
together. Explicit maintenance can still observe Session events. The
transport's Session subscriptions continue to support delivery across Turns;
the UI owns when to close them. Turn-limited subscriptions end at that Turn's
terminal fact and can miss later product notices. History notice IDs retain their historical position; live
notices absent from the loaded page stay visible without duplication. SystemNotice
never enters LiveTurnProjection or Provider continuation. UI projections are
bounded app-owned presentation state, not SDK or database truth.

Decode untrusted errors once at ingress. HTTP authentication responses also
unwrap the application envelope; a successful HTTP status alone is not a domain
response. Health uses its own documented shape. Electron main returns plain
envelopes, preload forwards them, and the renderer reconstructs typed errors.
Connection failure during bootstrap is not proof of logout.

Keep errors structured in UI/terminal state and translate at render time through
the existing presenter. Branch on codes/typed parameters, never English sentences.
Show failed Turns even without assistant text. Public error disclosure, report
ownership and protocol 0.4 migration are defined by [Error contract](ERROR_MODEL.md).

Desktop and Web settings follow ownership of the selected Computer. The central
Server's embedded Worker is claimed by its first administrator as a personal
Computer, using direct RPC; it is not a shared Computer. A competing claim must
preserve the current owner's registration and runtime status. Registration
uses the same UUID/credential contract for embedded and standalone Workers;
`direct`/`outbound` selects transport independently of ownership. See
[deployment and authorization](../../deploy/README.md#computer-identity-transport-and-configuration-access).

- SDKs own the complete product API, typed domain objects, structured errors,
  stream contracts, and transport-neutral lifecycle semantics.
- Transport adapters own serialization, authentication transport, connection
  lifecycle, upload encoding, and event delivery mechanics.
- Clients own presentation, interaction, optimistic state, and reconnect UX.
- The server owns search, matching, parsing, permission, and durable lifecycle rules.

Desktop's app-server controller owns backend restart and subscription replay.
Replay retains the verified account ID and last delivered cursor, never a token.
After replacement it reads the current session from the existing auth owner and
uses that account's current credential through normal request validation. Missing,
rejected or different-account credentials terminate recovery explicitly; there is
no anonymous fallback. Logout, a new login, unsubscribe and controller shutdown
retire subscription contexts; retired backend events are ignored. Original
renderer unsubscribe handles continue to resolve after backend replacement.
Subscriptions accept initial replay events before the subscribe response resumes;
only delivered events advance their cursor, never an unconsumed high-water mark.
Authenticated Desktop startup restores the existing host lifecycle through
`sync/status`, using the current login credential for enrollment and owner binding.
Sign-out retires subscriptions immediately and waits only for the backend
handshake, never pending replay. Recovery completion and auth callbacks affect
only their current bridge/account generation; a retired recovery cannot clear
its replacement or rebind the previous account.

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

## UI implementation

- UI state reflects actual backend state. Search, matching, parsing, and selection rules belong in the backend or existing shared modules, consistent with the ownership boundaries above.
- Product UI and runtime text changes require snapshot, visual, or E2E coverage in affected clients. With `insta`, generate, inspect, and accept expected snapshots; read `*.snap.new` or equivalent diffs before acceptance.
- Use existing TUI styling helpers and the default foreground unless the design system requires a specific color. Avoid hardcoded white and churn between equivalent span/line constructions.
- Use shared text-wrapping options and line utilities for wrapping, indentation, and list prefixes instead of custom string manipulation.

### Workspace inspection

Settings → Personalization → Workspace shows the configured default workspace
for the trusted current actor. `workspace/info` (RPC) and `GET /v1/workspace/info`
(HTTP) share `WorkspaceInfoRequest` / `WorkspaceInfoResponse`, with camelCase
`workspaceId`, `rootPrefix`, and `userWorkspaceDir`. The prefix is resolved against
runtime home by configuration; the user directory uses the same resolver as
workspace documents. `workspaceDocs.info()` in the HTTP SDK and `workspaceInfo()`
in the App Server SDK expose the same response. Desktop permits the empty request
through its existing IPC guard.

This read-only operation has the same actor access as workspace documents. It
accepts no user ID, workspace selector, or client path, creates no files, and
propagates configuration/identity errors through the unified error contract.
Existing sessions retain their own workspace bindings; this endpoint neither
selects a session's workspace nor manages workspace definitions. Extend selection
only together with an authorized product workspace management path.

## Computer selection and execution ownership

`features/computers` owns the independent rail, full paginated search, selection
and management view. Both clients use `sync/status.localComputerId` to identify
the connected management host's own Worker. On Desktop this is its local Computer;
on Web it is the Server's embedded Worker, not the browser's physical machine.
Ordinary accounts on a shared center do not see the center's private embedded
Worker. An authorized local Worker is selected by default; otherwise the saved
selection or an explicit Computer selection applies.
The existing account menu remains the account owner. Cloud/local is an explicit
host setting and an offline Computer never redirects execution elsewhere.

Product requests reuse canonical optional `nodeId` fields. The UI binds environment
APIs once at composition; Session operations retain their saved identity. Query
caches use account/control-plane/resource identity, with Computer identity for
environment and list views. Drafts/recent selection are client-owned and scoped;
Projects, Sessions and Timeline remain on the control plane. A Computer switch
unsubscribes observations without cancelling its running Turn.

`AgentHarness` and `WorkerEnvironment` hide embedded/direct/outbound transport.
Worker owns config, MCP credentials and Workspace files. Remote MCP OAuth callbacks
use the control-plane relay to the original bounded Worker attempt; only transient
state-hash routing lives on the control plane. SDK and the generated PB projection
reuse the canonical business types.

`ComputerDetails` presents the canonical `NodeInfo.systemInfo` report together
with existing owner, identity, heartbeat and registration metadata. The Worker
collects it once through `talos-system-info::collect()`, and the original Node
registration/store/sync paths carry that typed object. Missing optional hardware
stays unavailable. The report never decides permissions, identity or availability.
Computer management and synchronization controls stay on the dedicated right-hand
page; the business sidebar only shows the selected Computer and its resources.
