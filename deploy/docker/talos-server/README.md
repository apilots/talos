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

## Secrets and persistent settings

Uncomment only non-empty provider keys intentionally owned by the deployment.
Omit keys managed through the product UI: an injected empty value would otherwise
override the saved runtime value. Talos writes product settings under its
persistent `$TALOS_HOME`, including a separate `.env`. Do not bind mount the
operator-owned deployment `.env` over that runtime file.
