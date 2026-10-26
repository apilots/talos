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
installing their resources does not change authentication or provide an anonymous
desktop relay. The existing authenticated media ingress is still required.

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
