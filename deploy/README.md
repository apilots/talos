# Talos deployment

[Chinese](README_zh.md)

This directory owns deployment examples for `talos-server`, the standalone
`talos-worker`, and installation assets for the released `talos` terminal bundle.

## Layout

```text
deploy/
|-- caddy/          Caddy reverse-proxy example
|-- docker/         Docker Compose deployment
|-- install/        TUI release installer and documentation
|-- nginx/          Nginx reverse-proxy example
`-- ubuntu-offline/ Ubuntu offline Server, embedded Worker and TUI image
```

## Offline Ubuntu image

[`ubuntu-offline/`](ubuntu-offline/README.md) builds local `amd64` and `arm64`
offline bundles with a private deployment profile, packaged Browser resources,
diagnostic tools and a persistent-volume launcher. It reuses the formal runtime
resources and configuration upgrades; private profiles and image archives stay
out of Git.

## Server deployment

`talos-server` exposes HTTP APIs, the Web UI, and SSE Timeline streams. Reverse
proxies must disable response buffering and compression for
`/v1/sessions/*/events` so events are delivered incrementally.

Deploy standalone Workers with the dedicated `ghcr.io/apilots/talos-worker`
image, which uses outbound Connect and does not inherit the Server HTTP
healthcheck. Supply its persistent home, node UUID/credential and TLS trust as
described in the [Worker Compose example](docker/talos-worker/README.md) and
[Worker image guide](../build/worker/README.md). Talos releases
publish Server and Worker images for both `linux/amd64` and `linux/arm64`.

The examples under `caddy/`, `nginx/`, and `docker/` are the maintained starting
points. Review authentication, TLS, persistence, resource limits, and secrets
before production use.

Start the Docker Compose example after setting a provider key in its `.env`:

```bash
cd deploy/docker/talos-server
cp .env.example .env
docker compose up -d
curl http://127.0.0.1:8080/health
```

Before using the example, select an image version matching your SDK/UI and data
format. The Compose sample's pinned release is not a promise that every current
source-tree capability exists in that image.

For Talos Server at `agent.talos.love`, including Computer registration and RPC, use the
[Talos Server deployment guide](TALOS_SERVER.md), production profile and Compose file. The
general example above does not enable the registration listener.

## Computer identity, transport and configuration access

One central Talos Server issues immutable node UUIDs and registration credentials.
Every registered Computer belongs to a user, including embedded and standalone
Workers. Administrator directory access does not grant product execution access
to another user's Computer. The first administrator to sign in claims the Server's
embedded Worker; later logins preserve that personal owner, independently of
subsequent administrator role changes. It uses direct RPC
on the existing Server TLS endpoint, and private loopback for its own local calls.

Worker connection policy uses `agent_node.runner.transport` (`outbound` by
default, or `direct`) and the Server's node `endpoint`. An outbound Worker opens
an authenticated bidirectional Connect stream; it does not need a public listener.
A direct Worker needs a reachable TLS endpoint. Transport does not describe the
Computer's origin or change its owner. There is no deployment or Desktop
connection mode setting.

Configuration, models and MCP operations authorize the selected Computer owner.
Authentication remains controlled by `gateway.auth`; Sync preferences remain
independent of registration and local execution.

Managed `$TALOS_HOME/conf/talos.yaml` is normalized against the bundled template
before loading, with a backup before changes. Obsolete mode keys are removed;
node identity and credentials are preserved. Obsolete Desktop recovery flags are
removed without migrating their value. Explicit
`TALOS_CONFIG` files are operator-owned and skip normalization: remove obsolete
keys when updating those files, keeping the existing runner and credential files.
Direct registration and its TLS/authentication settings are validated together;
an upgrade that cannot preserve a saved Worker identity stops without rewriting
the configuration and requires an explicit repair.

Worker enrollment recovery is internal state at `$TALOS_HOME/.worker-state.json`;
`.worker-state.lock` serializes its updates under the existing configuration lock.
Both files are removed after the identity and protected credential are durable,
or after an existing saved identity successfully registers following interrupted
cleanup. Unknown CreateNode outcomes retain the recovery barrier and use the
existing owned-UUID recovery flow; known identities automatically register on
restart without provisioning another Computer or rotating their credentials.

## Data upgrades and rollback

Protocol 0.4 changes errors, Timeline payloads and history notices. Upgrade the
server, SDKs, Web, Desktop and raw-event consumers together; HTTP integrations
also need coordinated deployment. A successful health check is not protocol parity.
Before migration, stop writers and make a verified, access-controlled consistent
SQLite backup, retaining matching binaries/configuration and required Workspace
sources/Provider artifacts. Rollback restores the old backup and binaries and
loses later writes; binary-only rollback is unsafe. The authoritative migration
details are in [Error contract](../docs/architecture/ERROR_MODEL.md#upgrade-and-rollback).

The 0.1.8 repair is part of the SQLite startup upgrade and covers the legacy
Timeline tables, Provider JSONL artifacts, and `MEMORY.md` documents as one
validated operation. It retains immutable database and file sources before
removing legacy records, and its Product Upgrade task is retried after failure
or skipped after recorded success. Do not edit released SQLx checksums or run
this SQLite repair against another database backend. Validate an existing
history, a continued turn, a new turn, memory reflection, and restart behavior
before treating the upgrade as complete.

## Terminal installation

The release bundle installer lives under [`install/`](install/README.md). It
installs product resources separately from user-owned configuration and runtime
state.

Deployment changes require a real startup or installation path check, not only
static configuration validation.
