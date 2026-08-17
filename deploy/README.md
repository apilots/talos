# Talos deployment notes

## TUI installation

The versioned macOS/Linux TUI bundle installer is documented in
`deploy/install/README.md`. It installs product resources separately from the user-owned
`$TALOS_HOME` runtime directory.

## Reverse proxy requirements for SSE

Talos streams live timeline events through HTTP Server-Sent Events.

Current SSE route:

```text
/v1/sessions/*/events
```

Reverse proxies must handle this route separately from normal HTTP traffic:

- disable response compression for this route
- disable proxy buffering and proxy cache for this route
- flush upstream chunks immediately
- keep read timeouts long enough for active turns
- keep normal compression enabled for all non-SSE routes

The Talos server also sends SSE-friendly headers on this route:

- `Cache-Control: no-cache`
- `X-Accel-Buffering: no`

Use the examples in this directory as deployment references:

- `deploy/caddy/Caddyfile.example`
- `deploy/nginx/talos.conf.example`
- `deploy/docker/docker-compose.yml`
- `deploy/docker/.env.example`

If Talos is behind Cloudflare or another edge proxy, verify that the edge does not buffer,
compress, cache, or prematurely close the SSE route.
