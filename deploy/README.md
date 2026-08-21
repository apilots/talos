# Talos deployment

[Chinese](README_zh.md)

This directory owns deployment examples for `talos-server` and installation
assets for the released `talos` terminal bundle.

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

## Terminal installation

The release bundle installer lives under [`install/`](install/README.md). It
installs product resources separately from user-owned configuration and runtime
state.

Deployment changes require a real startup or installation path check, not only
static configuration validation.
