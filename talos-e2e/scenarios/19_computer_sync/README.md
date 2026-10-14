# Multi-control-plane Computer sync acceptance

`just e2e-group 19_computer_sync` starts a real center Server and Cloud Auth,
two independent Desktop controllers/app-server processes on macOS, a standalone
macOS Worker with a private direct TLS endpoint relayed by G, two standalone Linux Worker containers, and a separate Sync
container with SQLite. Docker Workers publish no inbound ports. All registration,
execution and synchronization use public APIs, original SDKs and WSS/TLS.

- `01_directory_registration`: real device login, own Computer identities,
  account-scoped directories, Linux/macOS hardware registration and identical
  metadata materialization, pure Workers without management databases.
- `02_execution_history`: funded real model/tool tasks on Desktop embedded,
  native Mac and Docker Linux Workers; another Desktop continues each Session on
  its immutable Computer, realtime progress, identical complete history and files
  only in the target Worker Workspace.
- `04_local_without_cloud`: cold DEV Server startup without Cloud Auth, G or H,
  real embedded Worker file writes, persistent history and continuation after restart,
  live Input admission and cancellation with no delayed tool side effects.
- `03_recovery_isolation`: first Sync outage before binding, account opt-out saved across a real Desktop restart, no auto-enable after H recovery, explicit re-enable; deployment YAML overrides every local account without changing another host; current-host/account opt-out without YAML changes, administrator/other-account and same-account/other-host isolation, local execution, durable Desktop identity
  and login across restart, pending outbox catch-up, real Sync container restart,
  pure Worker rename/restart without UUID change, another account's private
  Computer and Session remain absent from Alice's Desktop databases while G
  retains both accounts.

- `05_resource_interactions`: failed Direct registrations with an unreachable
  endpoint or invalid TLS hostname never advertise Connected; an actual B
  Coordinator Connect receives paused admission and disconnects before commit,
  restoring the complete original Worker marker. With B and H stopped, A still
  continues the same Session without an abandoned tool side effect or history.
  Original FileService source read/update/delete and
  explicit source-offline errors; cross-coordinator exact idempotent response,
  admission exclusion, queue/live input/cancellation, waiting answers and real
  child-agent callbacks with immutable Computer affinity and an explicit successful
  wait_agent terminal result; actual waiting cancellation while H is offline releases configuration after original terminal commit; waiting Workers reject configuration mutation; history clear advances
  the generation, and deleted parent/child trees stay deleted after a hub restart.
- `08_owner_configuration`: the owner changes model and Web Search settings on
  Desktop embedded, standalone macOS and Linux Docker Workers through the original
  RPC APIs; only the target Worker stores configuration and credentials. Other
  accounts cannot discover its configuration, and G private configuration remains
  administrator-only. Waiting embedded Sessions reject mutations via the internal
  alias, public UUID and another control plane, and still resume normally.
- `06_cloud_faults`: stop G, H and both independently; local execution remains
  available, durable outbox catches up, and a Worker restart preserves the Local
  history cursor barrier until the current coordinator's facts become Shared.
- `07_nat_routing`: two actual Server containers behind distinct Docker bridges,
  each with its own management DB. A test-only network-namespace firewall blocks
  unsolicited ingress; direct peer HTTP must fail. B continues a bound pure
  Worker's Session while the origin management service is stopped, and source
  FileService operations use the existing G relay after restoration.

Requires a real macOS host, Docker, the adjacent `talos-sync` source, a Cloud
fixture executable (`TALOS_CLOUD_SERVER_BIN` or `cloud-server` on `PATH`),
Node/pnpm, Python grpcio/protobuf/PyYAML, and existing DEV configuration.
Execution cases use the configured funded DeepSeek provider in `.env`; no model
key is baked into an image. Each runner invocation builds the current candidate images once, verifies their
exact image IDs before every case, and reuses release caches without accepting
stale images from a prior invocation. NAT additionally builds the current Server
image and a small test-only iptables helper; NET_ADMIN applies solely to these
isolated fixture network namespaces, never to the host or unrelated containers. `TALOS_SYNC_REPO`,
`TALOS_WORKER_E2E_IMAGE` and `TALOS_SYNC_E2E_IMAGE` select inputs.

Accounts are registered and authenticated through the real Cloud service; database
connections in assertions are read-only. All homes, credentials and volumes are
isolated under the E2E temporary root; logs and synthetic task history are saved
in the scenario report. `TALOS_MULTI_NODE_KEEP_TMP=1` retains homes for diagnosis.

This topology uses two Desktop runtimes on one physical Mac and real Linux Docker
containers. It does not claim cross-physical-device, K8s Pod, signed Desktop
installation or public production deployment coverage. PostgreSQL acceptance is a separate H storage matrix; the regular product group
uses SQLite. GUI screenshots are captured from the running product topology and
recorded separately in the joint Stage 5 evidence.

Set `TALOS_E2E_REVIEW_UI=1` for an optional observer WebUI during the real execution
case. Its URL and Computer UUIDs are recorded in `review-state.json`; authentication
uses the same real device approval API. Credentials remain in a private mode-600
fixture file, outside reports. The observer does not replace any scenario assertion
and is stopped with the fixture. Automated cases never wait for manual capture.
Screenshots must be taken while that topology is
alive; an older manual environment is not evidence for this group.

NAT acceptance re-applies and verifies ingress rules after restart using current
container addresses; real memory/reflection and child-agent callbacks return to
the current coordinator. Live Linux SQLite evidence is read from the Linux helper,
never opened concurrently by a Mac SQLite process.
