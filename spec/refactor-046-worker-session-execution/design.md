# Worker-owned Session execution

Status: draft

The bound Worker owns durable Session execution. The management runtime owns
coordination, product APIs and Timeline, Automation scheduling, and Sync.
They own separate databases. Existing pre-cutover history is readable only
on Computers that already hold a local copy.
Embedded and standalone deployments expose the same Worker execution
contract through different entry points. TUI and Desktop use the cloud
TalosServer product API for remote Computers. The cloud Server coordinates
those operations and calls the registered Workers.

1. [Execution ownership and management](execution-ownership.md) defines the
   core responsibilities, storage and event contracts, reconnect behavior,
   Automation, and rollout.
2. [Worker API entry points](worker-entrypoints.md) defines in-process calls
   for TUI, AppServer, and TalosServer, cloud Server ownership of remote
   operations, the Worker RPC adapter, and shared execution rules.
