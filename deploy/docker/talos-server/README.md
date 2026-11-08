# Talos Server

Use a published version in `.env`; the image repository and version are readable
deployment inputs. Both Compose profiles use the same `.env.example`.

~~~bash
cp .env.example .env
chmod 600 .env
~~~

## Basic Web/API profile

The default `docker-compose.yml` creates its own persistent volume and exposes
HTTP on loopback8080. Review the image's packaged configuration and set a provider
key through the product or a deliberately managed deployment environment entry.

~~~bash
docker compose config --quiet
docker compose up -d
~~~

## Production profile

`compose.production.yml` uses an existing external home volume, loopback8080/9443,
and read-only TLS files for authenticated Computer registration and RPC relay.
It requires the reviewed runtime configuration and certificates before startup.
`talos.yaml.example` lists shared runtime settings and the central TLS listener.
Both Server and Worker images package the shared `conf/talos-prod.yaml` as
their default `conf/talos.yaml`;
the production example additionally enables the central listener and TLS.
Preserve any saved Worker identity when applying settings to an existing home.
Use the [Talos Server production guide](../../TALOS_SERVER.md) for preparation,
proxy routing and acceptance.

Set `TALOS_SERVER_DATA_VOLUME` and `TALOS_SERVER_TLS_DIR` in `.env` to the existing
deployment resources. Select this profile explicitly when working in this example
directory:

~~~bash
docker compose -f compose.production.yml config --quiet
docker compose -f compose.production.yml up -d
~~~

For the maintained installation at `/opt/talos-server`, install this production
file as `/opt/talos-server/docker-compose.yml`, not the basic profile.

## Talo on a single Docker host

Talo is an opt-in managed AI assistant. Run Talos Server and its Talo Workers on
one Linux host with the same Docker Engine. Enable authentication and the
existing TLS Agent node control plane in the Server configuration. Use
`talos.worker.runtime` from [the sample configuration](talos.yaml.example).
Choose a published Worker image pinned by digest. Set
`control_plane_endpoint` to the HTTPS Agent ingress reachable from the Worker
network. The loopback-only ports published by this Compose file are available to
the host ingress, so the Worker uses the ingress hostname. For a private CA,
mount its certificate into Server and set `trusted_ca_file`.

Add the managed Worker overlay to the production profile. On the Linux host,
set `TALOS_DOCKER_GID` in `.env` to the result of
`stat -Lc '%g' /var/run/docker.sock`. If using a different host socket, set
`TALOS_DOCKER_SOCKET` and obtain the group ID from that socket instead. The
container-side path is `/var/run/docker.sock`. The supplemental group allows
the Server image's non-root user to access it.

~~~bash
docker compose -f compose.production.yml -f compose.managed-worker.yml config --quiet
docker compose -f compose.production.yml -f compose.managed-worker.yml up -d --no-build
~~~

To open this Server's Talo in native Electron, launch Talos Desktop with
`TALOS_DESKTOP_SERVER_URL=https://your-talos-server.example`. Sign in with the
same account used in Web. This mode opens the Server-hosted product in an
isolated Electron view. The existing desktop WebSocket uses the same Server
origin and the Worker connects out to the Server. Standalone Desktop continues
to use its local App Server.

The Docker socket grants host administration privileges. Apply the overlay to
a trusted Server deployment. Managed Workers use one Docker bridge with
inter-container connectivity disabled and keep their own Docker volumes.
Use a Linux Docker Engine that enforces bridge firewall rules.
They publish no host ports and have no Docker socket. Docker restarts
unexpectedly exited Workers with `unless-stopped`, while Server Stop and
sleep leave their containers stopped. Each Talo Worker
uses its own Provider configuration and API key. Docker supplies persistent
Home storage without a disk quota.

## Embedded Browser and terminal management

The image includes the embedded Worker's managed Chromium, Browser Use driver,
official MCP source snapshot and private container desktop resources. These are
the same runtime inputs as the [standalone Worker](../../../build/worker/README.md#managed-browser-boundary).
Both Compose profiles set 1 GiB private shared memory, the existing Chromium
seccomp profile and `no-new-privileges`. Their default profile path refers to
`../talos-worker/chromium-seccomp.json` in this repository. When copying deployment
files elsewhere, copy that profile too and set `TALOS_CHROMIUM_SECCOMP_PROFILE`
to its absolute deployed path. Do not use privileged mode or disable the sandbox.

The profiles do not impose a fixed process memory limit. Set a measured deployment
budget that accommodates the Server, embedded Worker and browser workload rather
than the old 1 GiB Server-only allowance. Browser/CDP/VNC listeners remain private;
installing their resources does not change authentication defaults. The embedded
Worker supports Browser tools, desktop viewing and control through the existing
same-origin WebSocket endpoint. Its in-process transport reuses the same bounded
media relay as registered Workers, without Cloud enrollment or a public VNC port.
With `gateway.auth=false`, the deployment's anonymous identity can view and control
only the local embedded desktop. With authentication enabled, viewers need a
verified local account context; logout closes active media. Registered remote
Workers continue to require their existing user authorization and live node lease.

The Server image also includes the `talos` CLI/TUI from the validated TUI product:

~~~bash
docker exec -it talos-server talos
docker exec talos-server talos status
~~~

The TUI starts its own embedded runtime using the same persistent Talos home and
settings. It is not a remote terminal frontend attached to the running Server.
Only Server starts automatically. Both images share the
[common diagnostic clients](../../../build/container/README.md); no SSH server,
cron service or privileged debugging process is started.

## Secrets and persistent settings

Uncomment only non-empty provider keys intentionally owned by the deployment.
Omit keys managed through the product UI: an injected empty value would otherwise
override the saved runtime value. Talos writes product settings under its
persistent `$TALOS_HOME`, including a separate `.env`. Do not bind mount the
operator-owned deployment `.env` over that runtime file.
