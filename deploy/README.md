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
`-- nginx/          Nginx reverse-proxy example
```

## Server deployment

`talos-server` exposes HTTP APIs, the Web UI, and SSE Timeline streams. Reverse
proxies must disable response buffering and compression for
`/v1/sessions/*/events` so events are delivered incrementally.

Deploy standalone Workers with the dedicated `ghcr.io/apilots/talos-worker`
image, which uses outbound Connect and does not inherit the Server HTTP
healthcheck. Supply its persistent home, node UUID/credential and TLS trust as
described in the [Worker image guide](../build/worker/README.md). Talos releases
publish Server and Worker images for both `linux/amd64` and `linux/arm64`.

The examples under `caddy/`, `nginx/`, and `docker/` are the maintained starting
points. Review authentication, TLS, persistence, resource limits, and secrets
before production use.

Start the Docker Compose example after setting a provider key in its `.env`:

```bash
cd deploy/docker
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

## Deployment mode and configuration access

`mode: local` is the default when omitted. `conf/talos-prod.yaml` explicitly uses
local mode with authentication; `conf/talos-cloud.yaml` uses cloud mode with
authentication. The current server Dockerfile packages the Cloud profile as
`/opt/talos/conf/talos.yaml`. Retained user configuration may differ: verify the
running instance's `/health` response, including `mode` and `auth_enabled`.

```text
Deployment profile --> server-owned mode --------+
                                                |
Request --> authentication --> trusted Actor ---+--> Gateway config guard
                                                        |
                         +------------------------------+------------------+
                         |                                                 |
                 open / local / admin                           cloud regular user
                         |                                                 |
                 configuration access                              PermissionDenied

Health mode + auth state --> Web settings visibility [UX, not enforcement]
```

| Authentication / deployment / role | Instance configuration management |
| --- | --- |
| Authentication disabled | Allowed; not a safe default for an exposed shared server |
| Local, authenticated user | Allowed without requiring administrator role |
| Cloud, authenticated administrator | Allowed |
| Cloud, authenticated regular user | Denied |

This gate covers configuration get/update/delete/reload, including LLM and web
search settings. It does not deny ordinary model listing or search execution.
Local mode does not bypass authentication when authentication is enabled.
Mode describes instance ownership, not Cloud Computer provisioning, execution
location or an Agent Provider choice. Choose local for a user-owned instance;
choose cloud for shared administration, then review credentials and role mapping.

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
