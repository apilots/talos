# Computer topology and account Sync observations

Status: implemented. Actual self-test and review evidence belongs in PR #28 and ignored root output; this document owns the reusable contract and verification strategy.

## Ownership and boundaries

Preserve the existing Computer rail, resource sidebar and management route. Management owns inspection selection independently of the active execution Computer. Opening a Computer explicitly selects it and enters its workspace; viewing topology or details never redirects execution.

The existing management Sync port owns authenticated account metrics retrieval. Verify the caller through the original AuthService and send its current Bearer token only to the configured Sync origin using HTTPS, the configured trust CA and no redirects. Account scope is derived by Sync and checked against the verified actor again on receipt. No account selector, anonymous dashboard request or browser-held Sync credential is introduced. Metrics reads do not enable synchronization or register a Worker.

Use the published `talos-sync-sdk` Metrics client and its canonical types for version 1 account observations. The SDK owns HTTPS transport, trust, deadline/body bounds and account/version validation; no direct HTTP adapter, copied DTO snapshot or sibling source checkout is needed. Talos retains its original verified login and product authorization owners. Convert upstream millisecond timestamps explicitly into Talos's canonical Unix seconds and expose one typed product model through HTTP, JSON-RPC, Rust/embedded and TypeScript Browser/Desktop SDKs. Errors retain the existing structured contract and safe reporting owner.

## Interface

The read-only `sync/metrics` and `GET /v1/sync/metrics` expose the authenticated account's current peer observations, safe Sync endpoint, freshness and resource counts. No request parameters select a user. Offline, unsupported or unauthorized telemetry fails explicitly; the UI retains Computer management and renders the Sync relationship as unknown.

Fixed Control Plane and Talos Sync nodes anchor the graph. Gray bidirectional execution/resource paths use existing Computer availability. Violet dashed bidirectional Sync paths require current upstream Computer presence mappings; a peer is not a Computer and one peer may advertise multiple Computers. Missing peer observations cannot identify the device's process role or prove remote convergence. Pure Workers have no direct Sync observation/edge. The fixed Control Plane-to-Sync relationship requires the current management host to be a Control Plane with its exact peer identity; a missing live observation renders that relationship unknown. Management runners publish only their verified own Computer aliases through the existing ephemeral presence SDK every ten seconds with a thirty-second expiry. Presence rooms close after each publication, so they do not reserve document-room capacity; pure Workers never run this management path. Publication runs as one connection-scoped future polled alongside the document session through Tokio select, so slow or missing presence replies cannot stall feed handling, uploads or management commands. It is cancelled with the connection and releases SDK room/upload guards before client shutdown; no detached worker or independent lifecycle is introduced. Publication failures are logged independently and never terminate otherwise functional document synchronization. After delayed enrollment, the original verified management owner refreshes the existing handle's ephemeral Computer observation set using Tokio watch; immutable durable capture aliases, execution selection and Worker lifetime remain unchanged.

Reuse paginated server-owned Computer search. Topology and list display the same loaded directory window, show loaded/total counts and preserve inspection. Load more remains explicit; do not present a partial window as the full account topology. Details and existing rename/status/recovery controls retain permissions and structured errors. Reuse Talos i18n for all Chinese/English labels and unknown/empty/loading/error states.

## Verification and compatibility

Cover authenticated retrieval, wrong-account replies, upstream errors, trust/redirect boundaries and timestamp conversion. Verify HTTP/RPC and SDK capability parity through generated protocol artifacts and focused tests. UI checks cover topology/list switching, inspection vs execution selection, presence-only Sync edges, pagination/search, failure visibility, both locales and responsive layout. Exercise real isolated Sync plus Talos where supported and report local topology acceptance separately from hosted CI. SDK integration and merging the reviewed PR are authorized; product release, production configuration and deployment remain independent.

## Enum-driven synchronization observations

One presentation enum owns unknown, online idle, recent document activity, disconnected, conflict, error, connecting, disabled and pending labels. Reuse SDK/types 0.1.3 account Metrics `lastHeartbeatAt`, converted from service milliseconds to product seconds. A live peer with no recent document activity is idle; heartbeat and presence never trigger document flow animation. Both Computer and Control Plane edges use the same activity rule. Receipt time is shown independently from document activity. Performance flush completeness is independent of live peer membership.

The existing local `sync/status` response exposes the current authenticated management `peerId` solely to scope existing connection/pending/error observations. Apply these observations only to that peer or the explicitly identified local Computer, including when telemetry fails; never attribute a local conflict to another Computer or report remote convergence. Typed conflict/idempotency-conflict errors override optimistic connected labels; other errors remain visible with their existing safe error presentation. Pending changes never count as confirmed transfers. Explicit disabled/connecting/disconnected states retain their meanings. Absent remote presence or unavailable/stale snapshots mean unknown, not disconnected; pure Workers still have no fabricated direct Sync edge. Live observations older than fifteen seconds relative to the client clock become unknown. Retain five-second queries; no SSE, new synchronization frame, persistent connection registry or remote error replication.

Verify graph/list/details/control consistency, idle-to-active-to-idle transitions, Control Plane motion, heartbeat versus activity, local conflict and pending scope, disconnect and recovery, unknown observations, both languages and reduced motion. Keep metrics transport and execution availability unchanged.

## Connection, activity and availability vocabulary

Keep the existing `SyncTopologyState` enum; do not introduce an `offline` Sync state.

| Presentation state | Evidence and meaning |
| --- | --- |
| `disconnected` — Sync disconnected | This management host explicitly reports a failed Sync transport connection. It is not a claim that the Sync service or a remote Computer is globally offline. |
| `connecting` — Connecting Sync | This host is attempting an initial connection or retry. A prior transport failure may remain visible during retry. |
| `unknown` — Sync unknown | No usable observation exists for this peer: missing mapping, unavailable telemetry, a failed status read, or a snapshot older than 15 seconds. Absence of evidence does not prove disconnection. |
| `idle` — Online, idle | Fresh observations contain a live peer connection and no document activity within 60 seconds. Heartbeat may continue; this does not prove convergence. |
| `syncing` — Recent sync activity | A fresh live peer observation contains document activity within 60 seconds. This indicates recent transfer activity, not completion of all pending work. |
| `pending` — Waiting to sync | This host reports queued changes. Pending work is not a confirmed transfer or acknowledgement. |
| `conflict` — Sync conflict | This host reports typed Conflict or IdempotencyConflict; retain pending work and the specific failure reason. CRDT merging does not automatically resolve invalid business facts. |
| `error` — Sync error | This host is connected but reports another synchronization failure. Connection liveness does not imply successful publication. |
| `disabled` — Sync is off | Sharing is explicitly disabled; this is not a transport outage. Local execution/capture remains independent. |

Local presentation precedence remains: disabled, typed conflict, connecting/disconnected, connected error, pending, then fresh activity/idle observations. If a connected host has no usable observation, show unknown rather than inventing activity. Pending counts may accompany a disconnected/connecting label.

Computer **offline** belongs to the existing directory/execution availability model. It describes an unavailable execution path; it does not establish physical power state or Sync transport state. Keep the execution and Sync labels separate in the same view. Local conflict, pending, disabled and connection evidence remains scoped to its exact host; never project it onto remote peers.

On a failed metrics refetch, retain only the last verified endpoint from the existing account/management-host query cache. The graph and service details may show that address while live observations, peer counts and resource statistics remain unavailable. No previous-account/host endpoint is shown after a scope change or logout; a successful read replaces it. No address has been verified before the first successful observation, so that address remains unknown.

The account Sync control owns one error notice for this page. A known Sync transport ServiceUnavailable/Timeout condition uses the contextual locale message; connection state remains a separate enum label. Other operation failures and conflicts preserve their specific structured error presentation. Telemetry and inspector views do not repeat the same failure. English source messages are not compared or parsed. This presentation rule does not alter the canonical ApiError, retry policy or synchronization protocol.
