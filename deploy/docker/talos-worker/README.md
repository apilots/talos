# Talos Worker

This example starts only the published `talos-worker` image. It has no WebUI,
management HTTP listener or Sync client. Dispatch and callbacks use outbound
Connect to Talos Server; no inbound port or public Worker domain is required.

## Prepare one owned Computer

1. Sign in to Talos Server and use its authenticated Computer creation API to
   obtain a Computer UUID and issued node credential. The Server determines the
   account owner; a Worker cannot select its owner or use a Cloud password as
   its node credential.
2. Copy `.env.example` to `.env`, set `TALOS_WORKER_NODE_ID` to that UUID and select
   the published image version. Keep `.env` private with mode 0600.
3. Prepare an isolated home and runtime config:

   ~~~bash
   mkdir -p home/conf
   cp talos.yaml.example home/conf/talos.yaml
   ~~~

4. Save the exact issued credential in `home/conf/node.credential`, with no
   quotes or variable assignment. Set the runtime provider key in `home/.env`
   (`TALOS_LLM_DEEPSEEK_API_KEY=...` for this example), or deliberately configure
   a non-empty deployment-owned key in `.env`.
5. Restrict `home/.env` and `home/conf/node.credential` to mode 0600. The published
   image runs as UID/GID 10001; make this isolated home writable and its files
   readable by that UID, for example with `sudo chown -R 10001:10001 ./home`.
   The bind mount intentionally refuses to create a missing home directory.

For a private registration endpoint, set `control_plane_endpoint` and
`trusted_ca_file` in the runtime YAML to that endpoint and its trusted CA. The
public `https://agent.talos.love` endpoint uses system trust.

The example lists the shared configuration fields. Its required `gateway` section
does not start HTTP in `talos-worker`; management, channel and Sync sections are
not consumed by this process. Execution tools, model settings and workspace paths
apply to this Worker. For direct transport, configure reachable Worker RPC TLS
and the Computer endpoint together; outbound requires no inbound listener.

## Validate and start on the chosen Worker host

~~~bash
docker compose config --quiet
docker compose pull talos-worker
docker compose up -d --no-build --pull never talos-worker
docker compose logs --tail 50 talos-worker
~~~

Check the Computer's availability through Talos Server. Do not use a Server HTTP
healthcheck for this outbound Worker. Retain the same home, UUID and credential
when replacing its container; do not run two live Workers with the same identity.
Use a different Compose project, isolated home and issued identity for each
additional Worker.

This directory is a deployment example, not part of the production Server
rollout. Nothing in the Server Compose files starts or deploys this Worker.
See the [Worker image guide](../../../build/worker/README.md) for image/runtime
details.

The [shared container runtime](../../../build/container/README.md) provides local
SSH, network, process and file diagnostic clients. These do not add an inbound
service, credentials, a TUI or extra container privileges.

### Browser runtime

Keep `chromium-seccomp.json` beside `docker-compose.yml`; the standard image's
managed Chromium requires the supplied user-namespace seccomp allowances and
private shared memory. The browser, virtual display and VNC processes start
lazily as the Worker user. No Worker port mapping is added. Browser/CDP/VNC
ports stay private to the container; the product viewer uses the configured
trusted Agent ingress and the authenticated same-origin WSS route. It does not
share the Docker host or a native Desktop screen.

See [Worker image browser boundary](../../../build/worker/README.md#managed-browser-boundary)
for pins, profile origin and sandbox requirements. Retain the Worker home when
replacing the container to preserve browser login and explicit pause choices.
