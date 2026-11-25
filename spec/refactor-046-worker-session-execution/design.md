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

## 2026-11-25 architecture decisions recorded / pending implementation

Recorded on 2026-11-25 against draft
210549f21dbfa0152ae76925646edda807fb4219. This file,
execution-ownership.md, and worker-entrypoints.md remain draft. This
section records the architecture decisions below and is pending
implementation. It is the basis for that implementation. Earlier text in
this file and in the linked specs stays as written. Where that text
differs, implement from this section.

### Unified Worker model

Native Worker, Desktop, and TUI share one Worker model and the Worker
kernel code. Each keeps only a thin adapter. All of their active outbound
registration and task and event transport use a bidirectional WebSocket
to TalosServer. Native Worker uses this model in place of the previous
gRPC relay. Desktop and TUI are self-contained: each includes a Worker
and its own UI, and can operate the local machine. Other machines are
ordinary Workers. A personal computer does not need a public inbound
port. The same-network cloud Talos Docker Worker in the deployment
exception below stays on RPC bidirectional streams.

### Worker authority and Server copy

The Worker is the authority for business and product data. TalosServer
holds a complete copy of all Worker data. The copy is read-only as a
business authority. A received replica is upserted into storage.
TalosServer updates the copy by cursor-based incremental pull and upsert.
Desktop, TUI, Web, and mobile read across machines directly from
TalosServer, without an independent Sync service.

Independent Sync may stay or be removed. This record leaves that choice
open. A retained Sync service replicates in one direction, from Worker to
TalosServer. If replication uses the control-plane WebSocket, the
independent service can be removed. A Session stays bound to its original
Worker. The design adds no cross-machine Session migration and no
bidirectional conflict merge.

### Deployment exception

A cloud Talos Docker Worker shares a network with TalosServer and still
uses RPC bidirectional streams. That deployment is not forced onto
WebSocket. Remote desktop screen streaming to a Talos container bypasses
Server relay. TalosServer only authorizes the stream and hands off the
address. The Desktop client connects directly to the container desktop
port. This streaming rule applies only to the container desktop stream.
It does not require a personal computer to open an inbound port.
