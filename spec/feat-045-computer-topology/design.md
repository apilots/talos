# Computer topology and account Sync observations

Status: implemented. Actual self-test and review evidence belongs in PR #28 and ignored root output; this document owns the reusable contract and verification strategy.

## Ownership and boundaries

Preserve the existing Computer rail, resource sidebar and management route. Management owns inspection selection independently of the active execution Computer. Opening a Computer explicitly selects it and enters its workspace; viewing topology or details never redirects execution.

The existing management Sync port owns authenticated account metrics retrieval. Verify the caller through the original AuthService and send its current Bearer token only to the configured Sync origin using HTTPS, the configured trust CA and no redirects. Account scope is derived by Sync and checked against the verified actor again on receipt. No account selector, anonymous dashboard request or browser-held Sync credential is introduced. Metrics reads do not enable synchronization or register a Worker.

Use the published `talos-sync-sdk` Metrics client and its canonical types for version 1 account observations. The SDK owns HTTPS transport, trust, deadline/body bounds and account/version validation; no direct HTTP adapter, copied DTO snapshot or sibling source checkout is needed. Talos retains its original verified login and product authorization owners. Convert upstream millisecond timestamps explicitly into Talos's canonical Unix seconds and expose one typed product model through HTTP, JSON-RPC, Rust/embedded and TypeScript Browser/Desktop SDKs. Errors retain the existing structured contract and safe reporting owner.

## Interface

The read-only `sync/metrics` and `GET /v1/sync/metrics` expose the authenticated account's current peer observations, safe Sync endpoint, freshness and resource counts. No request parameters select a user. Offline, unsupported or unauthorized telemetry fails explicitly; the UI retains Computer management and renders the Sync relationship as unknown.

Fixed Control Plane and Talos Sync nodes anchor the graph. Gray bidirectional execution/resource paths use existing Computer availability. Violet dashed bidirectional Sync paths require current upstream Computer presence mappings; a peer is not a Computer and one peer may advertise multiple Computers. Missing peer observations cannot identify the device's process role or prove remote convergence. Pure Workers have no direct Sync observation/edge. The fixed Control Plane-to-Sync relationship requires the current management host to be a Control Plane and its local peer to appear in authenticated observations; otherwise that relationship remains unknown. Management runners publish only their verified own Computer aliases through the existing ephemeral presence SDK every ten seconds with a thirty-second expiry. Presence rooms close after each publication, so they do not reserve document-room capacity; pure Workers never run this management path. Publication failures are logged independently and never terminate otherwise functional document synchronization. After delayed enrollment, the original verified management owner refreshes the existing handle's ephemeral Computer observation set using Tokio watch; immutable durable capture aliases, execution selection and Worker lifetime remain unchanged.

Reuse paginated server-owned Computer search. Topology and list display the same loaded directory window, show loaded/total counts and preserve inspection. Load more remains explicit; do not present a partial window as the full account topology. Details and existing rename/status/recovery controls retain permissions and structured errors. Reuse Talos i18n for all Chinese/English labels and unknown/empty/loading/error states.

## Verification and compatibility

Cover authenticated retrieval, wrong-account replies, upstream errors, trust/redirect boundaries and timestamp conversion. Verify HTTP/RPC and SDK capability parity through generated protocol artifacts and focused tests. UI checks cover topology/list switching, inspection vs execution selection, presence-only Sync edges, pagination/search, failure visibility, both locales and responsive layout. Exercise real isolated Sync plus Talos where supported and report local topology acceptance separately from hosted CI. SDK integration and merging the reviewed PR are authorized; product release, production configuration and deployment remain independent.
