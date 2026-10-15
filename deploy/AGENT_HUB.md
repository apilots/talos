# Computer registration hub production runbook

Scope: the Talos service under `/opt/talos-server` on `talos-host`. Apply this
runbook only in a separately authorized deployment window. Preparing files,
validating Compose and local acceptance do not authorize production changes.
Do not change Cloud, Sync, sibling service directories or global proxy settings.

## Ownership and release

| Endpoint | Owner |
| --- | --- |
| `api.talos.love` | Existing Cloud login, account/role and token introspection |
| `agent.talos.love` | One Talos Server instance: Computer directory, leases, Worker/Manager Connect and authenticated RPC/resource relay |
| `sync.talos.love/v1/sync` | Existing independent WSS account-data synchronization |

Desktop and Server keep their own management databases. The current coordinator
owns its Turn/Timeline; the hub forwards calls and callbacks. Pure Workers have
no management Session DB or Sync client. Desktop login automatically enrolls its
embedded Worker; standalone Workers receive an owned UUID and credential through
the authenticated Computer API. Registration never accepts a self-selected owner.

Use the published Server **0.1.9** manifest, not `latest` or a development build:

```text
ghcr.io/apilots/talos-server@sha256:5b5199794b9fabc06e3ba819c4e940e82ceff31a16e521d9ca6ff144c9c1f349
```

This amd64/arm64 digest is recorded in the release's `manifest.json`. No build or
new package is required. Keep HTTP and all gRPC services on the same instance;
a shared DB does not share live Registry/Connect handles across replicas.

## Deployment files

- [Production config](docker/talos-agent.yaml.example): complete minimal profile
  for this release, authenticated cloud mode, TLS control-plane listener on9443,
  private embedded Worker loopback listener, and the existing Cloud/Sync URLs.
- [Production Compose](docker/agent-hub.compose.yml): loopback8080/9443, external
  existing home volume, read-only TLS directory and current one-GiB resource cap.
  That cap is inherited from the observed deployment, not a measured capacity claim.
- [Non-secret environment](docker/.hub.env.example): exact image, existing volume
  name and absolute TLS directory. Existing provider secrets remain in `.env`.
- [Caddy](caddy/Caddyfile.example) or [Nginx](nginx/talos.conf.example): agent-only
  virtual host. Merge only that virtual host into the existing proxy; do not
  replace the whole proxy configuration or the API/Sync sites.

The minimal profile omits desktop-only MCP/browser commands. It retains the
ordinary Server runtime and private embedded Worker. Review the default DeepSeek
model/provider and any existing instance-specific resources before rollout;
private remote Workers retain their own model/search configuration and keys.
Do not copy a macOS PATH or browser executable into the Linux service.

## Persistence and configuration

The observed home volume is `talos-server_talos-data`, mounted at
`/home/talos/.talos`. Compose declares it **external** so a wrong/missing name
fails instead of silently creating an empty replacement. Confirm its exact name
and contents before any authorized application. Do not run `down -v` or prune.

Keep the whole home: `conf/`, `.env`, `data/` (SQLite and files), workspace data,
account/session caches and synchronization state. Preserve any existing node
UUID, credential and enrollment state when adapting retained configuration.
Never overlay an existing identity with the template's `runner: null`.

Install the reviewed configuration as `conf/talos.yaml` **inside that home**.
Keep its directory and file writable by UID/GID10001: account enrollment updates
YAML with atomic replacement. A read-only or single-file bind mount of the live
config prevents that lifecycle. TLS files are a separate read-only bind mount;
`create_host_path: false` prevents an accidental empty certificate directory.

Use the sample only as a new/reviewed configuration. Existing databases require
the coordinated backup/upgrade procedure in [deployment guidance](README.md#data-upgrades-and-rollback).
The runtime derives SQLite paths from `TALOS_HOME`; do not add
`storage.connection_string` or `logging.file` overrides.

## TLS and proxy routing

The public origin is `https://agent.talos.love:443`. Route
`/talos.agent.v1.*` to `https://127.0.0.1:9443` without stripping paths; this covers
NodeControl, NodeManagement, Coordinator, peer files and resource callbacks.
Other HTTP/Web requests go to8080; SSE `/v1/sessions/*/events` must flush without
buffering or compression. Preserve authentication metadata and bidirectional
HTTP/2 streams. The host proxy samples assume a proxy on the host, not in a
separate container where loopback points elsewhere.

The current control listener requires its own TLS identity. Prepare a private
upstream certificate with SAN `talos-agent.internal`, signed by the CA trusted by
the proxy. Store `server.crt`, `server.key` and `ca.crt` under
`/opt/talos-server/tls`; restrict the key to the service UID/group and allow the
proxy to read the CA. Keep the private CA signing key offline. Caddy verifies the
upstream name/CA; Nginx explicitly enables verification. Never disable validation.
Nginx also needs the existing public certificate at its documented example paths;
Caddy may retain its existing public certificate automation. Match the installed
proxy version and certificate management before applying a site fragment.

Worker/Desktop clients trust the public edge certificate using system roots;
leave client `trusted_ca_file` null for the public endpoint. The hub's
`trusted_node_ca_file` is for dialing direct Workers, not for authenticating the
reverse proxy. Outbound-only nodes need no public listener or private CA rollout.
The RPC listener reads its certificate at startup; certificate replacement needs
a controlled Talos restart, followed by reconnect acceptance.

The public endpoint currently traverses Cloudflare. Confirm gRPC is enabled and
TLS/HTTP2 ALPN is supported on443 before public acceptance. Cloudflare documents
403 responses when zone gRPC is disabled; the observed403 HTML is evidence of a
blocked path, not proof of the exact setting. Account-wide changes can affect
other sites and need separate review; this preparation does not change them.
See [Cloudflare gRPC requirements](https://developers.cloudflare.com/network/grpc-connections/),
[Caddy upstream TLS](https://caddyserver.com/docs/caddyfile/directives/reverse_proxy)
and [Nginx gRPC](https://nginx.org/en/docs/http/ngx_http_grpc_module.html).

## Read-only preparation and later rollout

1. Record the current container/image digest, Compose/env hashes, volume and health
   using only `/opt/talos-server` and its own container. Do not print secret values.
2. Prepare the new Compose, `.hub.env`, reviewed `conf/talos.yaml`, TLS identity and
   agent proxy fragment offline. Validate with the installed proxy version.
3. In a local disposable directory containing a placeholder `.env`, run:

   ```bash
   docker compose --project-name talos-server --env-file .hub.env \
     -f agent-hub.compose.yml config --quiet
   ```

   This only validates configuration. `.hub.env` comes from the non-secret sample;
   production `.env` must remain private. Do not paste resolved production config
   into reports because Compose can expand its provider secrets.
4. **Stop here without deployment authorization.** Later, stop writers for this
   Talos service, take a consistent access-controlled backup of its entire home,
   and verify that the backup restores in isolation with the original image.
   A live filesystem copy of SQLite/WAL is not a verified consistent backup.
5. During that later window, install only the reviewed files for this service,
   preserve the external volume, and start the pinned image with the same Compose
   project/service identity. Do not create a second writer on the existing volume.
   Confirm migrations/startup and then update only the agent proxy fragment.
6. Run the acceptance checklist below before declaring launch complete. Do not
   clear a stale history cursor, edit SQL, move a release tag or fall back to
   another Computer to make a check pass.

For rollback, stop writers and restore the **matching old home backup, config and
old image digest together**. Binary-only rollback after migration is unsafe;
restoring a backup loses later writes. Record that cutoff. Retain the failed
upgrade's data separately for investigation. Never restore over a live writer.

## Acceptance and launch gates

Use an explicitly authorized acceptance account and private isolated local homes.
Do not record passwords, access/refresh tokens, node credentials, device codes or
workspace contents in committed evidence. Use named disposable test resources;
clean up only those resources. Do not inject tokens/SQL or patch the application
to bypass a failed login, registration or history assertion.

| Gate | Required observation |
| --- | --- |
| Version/startup | Exact0.1.9 image, successful migrations; health reports cloud mode and auth enabled |
| Public transport | Valid certificate, HTTP2 ALPN, real SDK Register and both Connect streams through the public origin; HTTP health alone is insufficient |
| Desktop enrollment | Actual device login approval/polling, owned UUID/credential saved by the host; restart reuses identity without manual Computer creation |
| Pure Worker | Owner-created UUID/credential; outbound registration without a published port; lease/Connect reaches available |
| Lease/revocation | Several lease/renewal cycles, reconnect, disable and credential rotation; old credentials/streams rejected |
| Account boundary | Another authorized account cannot view/execute/rotate the first account's private Computer; unowned hub embedded execution rejected |
| Real execution | B dispatches to A on another physical machine/network; actual tool file stays on A; live progress, final result and callbacks return to B |
| Sync integration | Existing H shares complete facts; another management host continues the same Computer/Session after durable history is available |
| Independent faults | Sync disabled does not break current execution or registration; G offline makes remote routing unavailable while local execution remains usable |
| Restart/recovery | Hub restart preserves UUID/directory and reconnects; offline target remains explicit, no implicit reassignment or active-Turn takeover |
| Operations | Backup restore observed; disk/DB errors, authentication failures, connection/reconnect failures and certificate expiry have owned alerts |

Record image/source, topology, timestamps, actual assertions and limitations in
ignored root `output/`; summarize results and blockers in the deployment PR. Label local isolated
processes, Docker NAT and real public/physical-machine checks separately.
Unavailable infrastructure or product defects stay failed/blocked/not run; app
code changes require a separate task. Configuration/script repairs stay in the
production-preparation PR.
