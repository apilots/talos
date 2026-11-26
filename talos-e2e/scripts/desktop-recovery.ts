/** Secretless public regression: real stdio authentication and backend replacement. */
import assert from "node:assert/strict";
import { createServer } from "node:http";
import { createConnection, createServer as createSocket } from "node:net";
import { spawn, execFileSync } from "node:child_process";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import { randomUUID } from "node:crypto";
import { DatabaseSync } from "node:sqlite";
import { AppProtocol, TalosApiError } from "../../talos-app/ts-sdk/packages/core/src/index";
import { createDesktopMainAppServerBridge, type DesktopMainAppServerBridge } from "../../talos-app/ts-sdk/packages/app-server/src/desktop-main";
import { DesktopAppServerController } from "../../talos-app/desktop/src/app-server-controller";

const home = process.env.TALOS_HOME!;
const user = { user_id: "recovery-owner", username: "recovery", email: "recovery@example.invalid", role: "user", type: "free", disabled: false, created_at: 1, updated_at: 1 };
const token = randomUUID();
let introspections = 0;
let cloudAvailable = true;
let accountReads = 0;
let completions = 0;
let heldStream: { marker: string; opened: boolean; closed: boolean } | undefined;
const events: AppProtocol.TimelineEvent[] = [];
const auth = createServer(async (request, response) => {
  let body = "";
  for await (const chunk of request) body += chunk;
  if (request.url === "/chat/completions") {
    const completion = JSON.parse(body);
    if (completion.tools?.length) completions += 1;
    if (!completion.stream) {
      response.writeHead(200, { "content-type": "application/json" });
      response.end(JSON.stringify({ id: "fixture", object: "chat.completion", created: 1, model: "deepseek-chat", choices: [{ index: 0, message: { role: "assistant", content: '{"operation":"noop"}' }, finish_reason: "stop" }], usage: { prompt_tokens: 5, completion_tokens: 2, total_tokens: 7 } }));
      return;
    }
    const chunk = { id: "fixture", object: "chat.completion.chunk", created: 1, model: "deepseek-chat", choices: [{ index: 0, delta: { role: "assistant", content: completion.tools?.length ? "recovery response" : '{"operation":"noop"}' }, finish_reason: null }] };
    const end = { ...chunk, choices: [{ index: 0, delta: {}, finish_reason: "stop" }], usage: { prompt_tokens: 5, completion_tokens: 2, total_tokens: 7 } };
    response.writeHead(200, { "content-type": "text/event-stream" });
    if (heldStream && JSON.stringify(completion.messages?.findLast((message: { role: string }) => message.role === "user"))?.includes(heldStream.marker)) {
      const active = heldStream;
      response.on("close", () => { active.closed = true; });
      active.opened = true;
      response.write(`data: ${JSON.stringify(chunk)}\n\n`);
      return;
    }
    response.end(`data: ${JSON.stringify(chunk)}\n\ndata: ${JSON.stringify(end)}\n\ndata: [DONE]\n\n`);
    return;
  }
  if (request.url === "/v1/me") {
    accountReads += 1;
    if (!cloudAvailable) { response.writeHead(503).end(); return; }
    if (request.headers.authorization !== `Bearer ${token}`) { response.writeHead(401).end(); return; }
    response.setHeader("content-type", "application/json");
    response.end(JSON.stringify({ ...user, status: "active", email_verified: true, set_password_required: false, created_at_ms: 1, updated_at_ms: 1 }));
    return;
  }
  if (request.url !== "/v1/auth/introspect") { response.writeHead(404).end(); return; }
  introspections += 1;
  if (!cloudAvailable) { response.writeHead(503).end(); return; }
  response.setHeader("content-type", "application/json");
  response.end(JSON.stringify({ active: JSON.parse(body).token === token,
    user: { ...user, status: "active" }, capabilities: { is_admin: false, role: "user", scopes: ["sessions:read", "sessions:write", "projects:read", "projects:write"] },
    entitlement: { can_use: true, plan: "free", reason: null }, expires_at_ms: null }));
});
await new Promise<void>((resolve) => auth.listen(0, "127.0.0.1", resolve));
let controllerToClose: DesktopAppServerController | undefined;
let serverToClose: ReturnType<typeof spawn> | undefined;
let hubToClose: ReturnType<typeof spawn> | undefined;
try {
  const address = auth.address();
  assert(address && typeof address !== "string");
  const api = `http://127.0.0.1:${address.port}`;
  const configPath = process.env.TALOS_CONFIG!;
  const config = JSON.parse(await readFile(configPath, "utf8"));
  config.timezone = "${TALOS_DESKTOP_RECOVERY_TIMEZONE}";
  process.env.TALOS_DESKTOP_RECOVERY_TIMEZONE = "America/New_York";
  config.gateway.auth = true;
  config.cloud.api_base_url = api;
  const hubBinary = process.env.TALOS_E2E_SYNC_HUB_BIN;
  config.sync.enabled = Boolean(hubBinary);
  config.llm.provider = "deepseek";
  config.llm.model = "deepseek-chat";
  config.agent.inner_model = { provider: "deepseek", id: "deepseek-chat" };
  config.llm.deepseek.base_url = api;
  config.llm.deepseek.api_key = "controlled-fixture";
  config.llm.deepseek.models = [{ id: "deepseek-chat", name: "Controlled recovery", context_window: 65536, reasoning: false, input: ["text"], cost: {} }];
  const rpcPort = await freePort();
  const httpPort = await freePort();
  const cert = path.join(home, "tls.crt");
  const key = path.join(home, "tls.key");
  execFileSync("openssl", ["req", "-x509", "-newkey", "rsa:2048", "-nodes", "-keyout", key, "-out", cert, "-days", "1", "-subj", "/CN=127.0.0.1", "-addext", "subjectAltName=IP:127.0.0.1", "-addext", "basicConstraints=critical,CA:FALSE", "-addext", "keyUsage=critical,digitalSignature,keyEncipherment", "-addext", "extendedKeyUsage=serverAuth"], { stdio: "ignore" });
  config.cloud.agent_base_url = `https://127.0.0.1:${rpcPort}`;
  config.cloud.agent_trusted_ca_file = cert;
  config.agent_node = {};
  const hubDatabase = path.join(home, "hub.db");
  let startHub: (() => Promise<void>) | undefined;
  if (hubBinary) {
    const hubPort = await freePort();
    const hubConfig = path.join(home, "sync.yaml");
    await writeFile(hubConfig, JSON.stringify({
      listener: { bind_address: `127.0.0.1:${hubPort}`, certificate_file: cert, private_key_file: key },
      storage: { backend: "sqlite", path: hubDatabase, max_connections: 4, busy_timeout_ms: 30000 },
      auth: { issuer_url: api, timeout_ms: 5000, revalidate_interval_ms: 1000 },
    }));
    startHub = async () => {
      hubToClose = spawn(hubBinary, ["--config", hubConfig], { stdio: ["ignore", "inherit", "inherit"] });
      await waitFor(() => new Promise<boolean>((resolve) => {
        const socket = createConnection({ host: "127.0.0.1", port: hubPort });
        socket.once("connect", () => { socket.destroy(); resolve(true); });
        socket.once("error", () => resolve(false));
      }));
    };
    await startHub();
    config.sync.hub_endpoint = `wss://127.0.0.1:${hubPort}/v1/sync`;
    config.sync.trusted_ca_file = cert;
  }
  await writeFile(configPath, JSON.stringify(config));
  const serverHome = path.join(home, "server");
  await mkdir(path.join(serverHome, "conf"), { recursive: true });
  const serverConfig = structuredClone(config);
  serverConfig.sync.enabled = false;
  serverConfig.gateway.port = httpPort;
  serverConfig.agent_node = { control_plane: { bind_address: `127.0.0.1:${rpcPort}`, tls_certificate_file: cert, tls_private_key_file: key } };
  const serverPath = path.join(serverHome, "selected-config.yaml");
  await writeFile(serverPath, JSON.stringify(serverConfig));
  const server = spawn(process.env.TALOS_SERVER_BIN!, [], { env: { ...process.env, TALOS_HOME: serverHome, TALOS_CONFIG: serverPath, TALOS_DESKTOP_HOSTNAME: undefined }, stdio: ["ignore", "ignore", "inherit"] });
  serverToClose = server;
  await waitFor(async () => { try { return (await fetch(`http://127.0.0.1:${httpPort}/health`)).ok; } catch { return false; } });
  await mkdir(path.join(home, "auth"), { recursive: true });
  await writeFile(path.join(home, "auth/cloud-session.json"), JSON.stringify({ schema_version: 2, cloud_api_base_url: api,
    access_token: token, token_type: "Bearer", expires_at_ms: Date.now() + 3600000, last_used_at_ms: Date.now(), user }), { mode: 0o600 });
  const bridges: DesktopMainAppServerBridge[] = [];
  const requests: Array<{ method: string; params: unknown; authorized: boolean }> = [];
  const errors: Array<AppProtocol.ErrorCode | null> = [];
  const terminations: AppProtocol.SubscriptionTerminatedNotification[] = [];
  const controller = new DesktopAppServerController({
    command: process.env.TALOS_APP_SERVER_BIN, env: { ...process.env, TALOS_DESKTOP_HOSTNAME: "recovery-desktop" }, desktopHost: true, clientKind: "desktop",
    restartDelayMs: 10,
    bridgeFactory: (options) => {
      const bridge = createDesktopMainAppServerBridge(options);
      const invoke: typeof bridge.invoke = bridge.invoke.bind(bridge);
      bridge.invoke = <T>(method: AppProtocol.ClientMethod, params: unknown, options?: { actorCredentials?: AppProtocol.ActorCredentials | null }) => {
        requests.push({ method, params, authorized: options?.actorCredentials?.credential.token === token });
        return invoke<T>(method, params, options);
      };
      bridges.push(bridge);
      return bridge;
    },
    onEvent: (event) => events.push(event),
    onSubscriptionTerminated: (notification) => terminations.push(notification),
    onRuntimeState: (state) => { if (state.kind === "error") errors.push(state.error?.code ?? null); },
  });
  controllerToClose = controller;
  await controller.connect();
  // Desktop enrollment intentionally owns its managed config path. The Server
  // also verifies startup settings selected through a different TALOS_CONFIG.
  for (const logHome of [home, serverHome]) {
    let startupTimestamp = "";
    await waitFor(async () => {
      startupTimestamp = (await readFile(path.join(logHome, "logs/talos.log"), "utf8")).split(/\s+/)[0] ?? "";
      return startupTimestamp.length > 0;
    });
    const expectedOffset = new Intl.DateTimeFormat("en-US", { timeZone: "America/New_York", timeZoneName: "longOffset" })
      .formatToParts(new Date(startupTimestamp)).find((part) => part.type === "timeZoneName")!.value.replace("GMT", "");
    assert(startupTimestamp.endsWith(expectedOffset), `Startup log did not use the configured root timezone: ${startupTimestamp}`);
  }
  console.log("CASE desktop_and_selected_server_config_timezone PASS");
  const options = { actorCredentials: { credential: { type: "existing_session_token" as const, token } } };
  await controller.invoke("user/current", {}, options);
  const params = { sessionId: randomUUID(), turnId: null, sinceEventId: null, replayLimit: 50, closeWhenIdle: false, closeOnTerminal: false };
  // Status is a local snapshot. Execution starts after background enrollment
  // establishes this account's Computer ownership, independently of polling.
  const computerId = await waitForOwnedComputer(controller, options);
  const turn = { sessionId: params.sessionId, input: { content: [{ type: "text", text: "before restart" }] }, modelSelection: null, userPermissionMode: "fullAccess", projectId: null, currentDirectory: null, clientRequestId: randomUUID() };
  await controller.invoke("turn/start", turn, options);
  await waitFor(async () => (await controller.invoke<AppProtocol.SessionHistoryResponse>("session/history", { sessionId: params.sessionId, limit: 50, cursor: null }, options)).data.length === 1);
  await assert.rejects(bridges[0]!.invoke("event/subscribe", params), (error) => error instanceof TalosApiError && error.code === "unauthenticated");
  const subscribed = await controller.invoke<AppProtocol.SubscribeTimelineResponse>("event/subscribe", params, options);
  await waitFor(async () => events.some((event) => event.event.type === "agent" && event.event.data.payload.type === "turnCompleted"));
  const cursor = events.at(-1)!.eventId;
  const identity = await controller.invoke<AppProtocol.DesktopWorkerIdentityResponse>("desktop/workerIdentity", null);
  assert.equal(identity.nodeId, computerId);
  const beforeIds = events.map((event) => event.eventId);
  assert(bridges[0]!.client.transport.terminate("SIGTERM"));
  const deadline = Date.now() + 30000;
  while (bridges.length < 2 && Date.now() < deadline) {
    await new Promise((resolve) => setTimeout(resolve, 20));
  }
  await controller.invoke("user/current", {}, options);
  assert.deepEqual(await controller.invoke("desktop/workerIdentity", null), identity);
  assert.equal(await waitForOwnedComputer(controller, options), computerId);
  await waitFor(async () => {
    const response = await fetch(`http://127.0.0.1:${httpPort}/v1/computers/${identity.nodeId}`, { headers: { Authorization: `Bearer ${token}` } });
    return (await response.json()).data?.available === true;
  });
  await controller.invoke("turn/start", { ...turn, input: { content: [{ type: "text", text: "after restart" }] }, clientRequestId: randomUUID() }, options);
  await waitFor(async () => events.filter((event) => event.event.type === "agent" && event.event.data.payload.type === "turnCompleted").length === 2);
  assert.equal(completions, 2);
  assert.equal(new Set(events.map((event) => event.eventId)).size, events.length);
  assert.deepEqual(events.slice(0, beforeIds.length).map((event) => event.eventId), beforeIds);
  assert.equal((requests.filter((request) => request.method === "event/subscribe")[2]!.params as AppProtocol.SubscribeTimelineRequest).sinceEventId, cursor);
  const recovered = await controller.invoke<AppProtocol.RecoverTimelineResponse>("timeline/recover", { sessionId: params.sessionId, afterEventId: null, replayLimit: 100 }, options);
  assert.deepEqual(events.map((event) => event.eventId), recovered.replay.events.map((event) => event.eventId));
  const removed = await controller.invoke<AppProtocol.EventUnsubscribeResponse>("event/unsubscribe", { subscriptionId: subscribed.subscriptionId }, options);
  // Registration may briefly report an unavailable outbound Computer while its new stream publishes.
  assert(errors.every((code) => code === AppProtocol.ErrorCode.ServiceUnavailable), `Recovery failed: ${errors.join(",")}`);
  assert.deepEqual(terminations, []);
  assert.equal(bridges.length, 2);
  assert.equal(removed.unsubscribed, true);
  assert.notEqual(removed.subscriptionId, subscribed.subscriptionId);
  assert.deepEqual(requests.filter((request) => request.method === "event/subscribe").map((request) => request.authorized), [false, true, true]);
  if (startHub) {
    const syncStatus = () => controller.invoke<AppProtocol.SyncStatusResponse>("sync/status", null, options);
    const isolationId = randomUUID();
    const startTurn = (text: string) => controller.invoke<AppProtocol.StartTurnResponse>("turn/start", {
      ...turn, sessionId: isolationId, input: { content: [{ type: "text", text }] }, clientRequestId: randomUUID(),
    }, options);
    const timeline = async () => (await controller.invoke<AppProtocol.RecoverTimelineResponse>("timeline/recover", {
      sessionId: isolationId, afterEventId: null, replayLimit: 200,
    }, options)).replay.events;
    const terminal = async (turnId: string) => (await timeline()).filter(({ event }) => (
      event.type === "agent" && event.data.identity.turnId === turnId
      && ["turnCompleted", "turnFailed"].includes(event.data.payload.type)
    ));
    const waitForCompletion = async (turnId: string) => {
      await waitFor(async () => (await terminal(turnId)).length === 1, 5000);
      const [entry] = await terminal(turnId);
      assert(entry?.event.type === "agent" && entry.event.data.payload.type === "turnCompleted");
    };
    const runWhileBlocked = async (scenario: string) => {
      const began = performance.now();
      await waitForCompletion((await startTurn(`finish while ${scenario}`)).turnId);
      const finishMs = performance.now() - began;
      assert(finishMs < 5000, `Local completion took ${finishMs.toFixed(1)}ms`);
      const marker = `hold ${scenario} ${randomUUID()}`;
      const active = { marker, opened: false, closed: false };
      heldStream = active;
      try {
        const started = await startTurn(marker);
        await waitFor(async () => active.opened, 5000);
        const stoppedAt = performance.now();
        const stopped = await controller.invoke<AppProtocol.InterruptTurnResponse>("turn/interrupt", {
          identity: { sessionId: isolationId, turnId: started.turnId },
        }, options);
        assert.equal(stopped.outcome.type, "cancelled");
        await waitFor(async () => active.closed && (await terminal(started.turnId)).length === 1, 5000);
        const stopMs = performance.now() - stoppedAt;
        assert(stopMs < 5000, `Local Stop took ${stopMs.toFixed(1)}ms`);
        const [entry] = await terminal(started.turnId);
        assert(entry?.event.type === "agent" && entry.event.data.payload.type === "turnFailed");
        assert.equal(entry.event.data.payload.data.error.code, "cancelled");
        const recovered = await controller.invoke<AppProtocol.SessionActiveTurnResponse>("session/active_turn", { sessionId: isolationId }, options);
        assert.equal(recovered.activeTurn, null);
        const cancelledEvents = async () => (await timeline()).filter(({ event }) =>
          event.type === "agent" && event.data.identity.turnId === started.turnId,
        ).map(({ eventId }) => eventId);
        const stoppedEvents = await cancelledEvents();
        await waitForCompletion((await startTurn(`follow-up after ${scenario}`)).turnId);
        assert.equal((await terminal(started.turnId)).length, 1);
        assert.deepEqual(await cancelledEvents(), stoppedEvents);
        console.log(`CASE desktop_sync_${scenario}_local_completion_and_stop PASS finish=${finishMs.toFixed(1)}ms stop=${stopMs.toFixed(1)}ms`);
      } finally {
        heldStream = undefined;
      }
    };
    const storage = new DatabaseSync(path.join(home, "data", "talos.db"));
    const pending = () => (storage.prepare("SELECT COUNT(*) AS count FROM visible_sync_outbox").get() as { count: number }).count;
    const batches = 379;
    const noticesPerBatch = process.env.TALOS_E2E_SYNC_STRESS === "1" ? 32 : 0;
    try {
      await waitFor(async () => (await syncStatus()).connection === "connected");
      const baseline = await startTurn("publish baseline before Sync faults");
      await waitForCompletion(baseline.turnId);
      await waitFor(async () => pending() === 0 && storage.prepare(
        "SELECT 1 FROM sync_documents WHERE kind='session' AND resource_id=? AND session_projection_json IS NOT NULL",
      ).get(isolationId) !== undefined);
      await stopChild(hubToClose);
      await waitFor(async () => (await syncStatus()).connection === "disconnected");
      await runWhileBlocked("offline");
      assert(pending() > 0, "offline local work must leave durable Sync changes");

      // Hold the actual Hub writer so a live WSS transport retains its outbox.
      await startHub();
      await waitFor(async () => (await syncStatus()).connection === "connected");
      const hubWriter = new DatabaseSync(hubDatabase);
      const seedPending = storage.prepare("SELECT COUNT(*) AS count FROM visible_sync_outbox WHERE seq > ? AND seq <= ?");
      let seedAfter: number | bigint = 0;
      let seedThrough: number | bigint = 0;
      const remainingSeed = () => (seedPending.get(seedAfter, seedThrough) as { count: number }).count;
      try {
        await waitFor(async () => {
          try { hubWriter.exec("BEGIN IMMEDIATE"); return true; }
          catch (error) {
            if (error instanceof Error && "errcode" in error && error.errcode === 5) return false;
            throw error;
          }
        }, 5000);
        const binding = storage.prepare("SELECT hub_id,account_id,source_user_id FROM sync_bindings").get();
        const peerId = storage.prepare("SELECT peer_id FROM sync_identity").get()?.peer_id;
        const document = storage.prepare("SELECT session_projection_json FROM sync_documents WHERE kind='session' AND resource_id=?").get(isolationId);
        assert(binding && typeof peerId === "string" && typeof document?.session_projection_json === "string");
        let before = JSON.parse(document.session_projection_json).snapshot;
        const append = storage.prepare("INSERT INTO sync_outbox(hub_id,account_id,source_user_id,kind,resource_id,changes_json,created_at) VALUES (?,?,?,'session',?,?,?)");
        storage.exec("PRAGMA busy_timeout=5000; BEGIN IMMEDIATE");
        try {
          seedAfter = Number(storage.prepare("SELECT COALESCE(MAX(seq), 0) AS seq FROM sync_outbox").get()?.seq);
          for (let index = 0; index < batches; index++) {
            const after = { ...before, session: { ...before.session, title: `backlog ${index}` } };
            const metadata = (data: unknown) => ({ kind: "session", data });
            const changes: unknown[] = [{ type: "metadata", data: { before: metadata(before), after: metadata(after) } }];
            for (let notice = 0; notice < noticesPerBatch; notice++) {
              const id = `stress-${index}-${notice}`;
              const persistedAt = Date.now();
              changes.push({ type: "event", data: {
                key: { type: "notice", peerId, sourceEventId: id }, persistedAt,
                event: { type: "systemNotice", data: {
                  identity: { sessionId: isolationId, turnId: baseline.turnId }, occurredAt: persistedAt,
                  notice: { level: "info", content: { type: "text", data: id } },
                } },
              } });
            }
            seedThrough = append.run(binding.hub_id!, binding.account_id!, binding.source_user_id!, isolationId,
              JSON.stringify(changes), Math.floor(Date.now() / 1000)).lastInsertRowid;
            before = after;
          }
          storage.exec("COMMIT");
        } catch (error) {
          storage.exec("ROLLBACK");
          throw error;
        }
        assert.equal(remainingSeed(), batches);
        console.log(`CASE desktop_sync_seeded_backlog PASS batches=${batches} timelineFacts=${batches * noticesPerBatch}`);
        await runWhileBlocked("backlog");
        assert.equal(remainingSeed(), batches, "the Hub must still be blocked after finish and Stop");
      } finally {
        hubWriter.close();
      }
      const drainStarted = performance.now();
      const drainBudget = noticesPerBatch ? 120000 : 30000;
      const remainingBudget = () => drainBudget - (performance.now() - drainStarted);
      if (noticesPerBatch) {
        await waitFor(async () => {
          const rows = remainingSeed();
          return rows > 0 && rows < batches;
        }, remainingBudget());
        await runWhileBlocked("recovery");
        // Foreground turns add their own rows, so use only the seeded prefix.
        const rows = remainingSeed();
        assert(rows > 0, "seeded work must remain after recovery finish and Stop");
        console.log(`CASE desktop_sync_recovery_overlap PASS seededBatchesRemaining=${rows}`);
      }
      await waitFor(async () => pending() === 0, remainingBudget());
      console.log(`CASE desktop_sync_backlog_drained PASS elapsed=${(performance.now() - drainStarted).toFixed(1)}ms`);
    } finally {
      storage.close();
    }
  }
  // Both gateways retain authorization locally. Cloud failure must not add
  // introspection, delay metadata/History/status, or prevent local idle deletion.
  const httpOrigin = `http://127.0.0.1:${httpPort}`;
  const headers = { Authorization: `Bearer ${token}` };
  assert((await fetch(`${httpOrigin}/v1/user`, { headers })).ok);
  const beforeOffline = { introspections, accountReads };
  cloudAvailable = false;
  const writer = new DatabaseSync(path.join(home, "data", "talos.db"));
  // Post-Turn effects may still own a short write transaction. Acquire the
  // fixture lock before timing reads, yielding so the HTTP fixture can settle.
  await waitFor(async () => {
    try { writer.exec("BEGIN IMMEDIATE"); return true; }
    catch (error) {
      if (error instanceof Error && "errcode" in error && error.errcode === 5) return false;
      throw error;
    }
  }, 2000).catch((error: unknown) => { writer.close(); throw error; });
  const started = performance.now();
  let readDeadline: ReturnType<typeof setTimeout> | undefined;
  try {
    const reads = Promise.all([
      controller.invoke<AppProtocol.SessionHistoryResponse>("session/history", { sessionId: params.sessionId, limit: null, cursor: null }, options),
      controller.invoke("session/get", { sessionId: params.sessionId }, options),
      controller.invoke("session/list", { nodeId: null, limit: null, offset: null, projectId: null }, options),
      controller.invoke("sync/status", null, options),
      fetch(`${httpOrigin}/v1/sync/status`, { headers }),
      controller.invoke("session/active_turn", { sessionId: params.sessionId }, options),
    ]);
    const localReads = await Promise.race([reads, new Promise<never>((_resolve, reject) => {
      readDeadline = setTimeout(() => reject(new Error("Local Desktop reads waited for the Sync writer.")), 2000);
    })]);
    assert.equal(localReads[0].data.length, 2);
    assert(localReads[4].ok);
    assert(performance.now() - started < 2000, "offline local Desktop/Web reads stalled");
    const offlineLive = await controller.invoke<AppProtocol.SubscribeTimelineResponse>("event/subscribe", { ...params, sinceEventId: events.at(-1)!.eventId }, options);
    await controller.invoke("event/unsubscribe", { subscriptionId: offlineLive.subscriptionId }, options);
    console.log(`CASE desktop_sync_writer_local_reads PASS ${(performance.now() - started).toFixed(1)}ms`);
  } finally {
    clearTimeout(readDeadline);
    writer.exec("ROLLBACK");
    writer.close();
  }
  await controller.invoke("session/delete", { sessionId: params.sessionId }, options);
  assert.deepEqual({ introspections, accountReads }, beforeOffline, "local reads/Live/deletion must not call Cloud");
  cloudAvailable = true;
  console.log(`CASE desktop_cloud_offline_local_reads_and_idle_delete PASS ${(performance.now() - started).toFixed(1)}ms`);
  assert(introspections > 0);
  console.log("CASE desktop_authenticated_backend_restart PASS");
} finally {
  try { await controllerToClose?.close(); }
  finally {
    await stopChild(hubToClose);
    if (serverToClose && serverToClose.exitCode === null && serverToClose.signalCode === null) {
      const exited = new Promise<void>((resolve) => serverToClose!.once("exit", () => resolve()));
      serverToClose.kill("SIGINT");
      await exited;
    }
    await new Promise<void>((resolve, reject) => auth.close((error) => error ? reject(error) : resolve()));
  }
}

async function stopChild(child: ReturnType<typeof spawn> | undefined) {
  if (!child || child.exitCode !== null || child.signalCode !== null) return;
  const exited = new Promise<void>((resolve) => child.once("exit", () => resolve()));
  child.kill("SIGINT");
  await exited;
}

async function freePort(): Promise<number> {
  const socket = createSocket();
  await new Promise<void>((resolve) => socket.listen(0, "127.0.0.1", resolve));
  const address = socket.address();
  assert(address && typeof address !== "string");
  await new Promise<void>((resolve) => socket.close(() => resolve()));
  return address.port;
}
async function waitForOwnedComputer(
  controller: DesktopAppServerController,
  options: { actorCredentials: AppProtocol.ActorCredentials },
): Promise<string> {
  let computerId: string | null = null;
  await waitFor(async () => {
    const status = await controller.invoke<AppProtocol.SyncStatusResponse>("sync/status", null, options);
    computerId = status.localComputerId;
    return computerId !== null && computerId !== "embedded";
  });
  assert(computerId);
  return computerId;
}
async function waitFor(probe: () => Promise<boolean>, timeoutMs = 30000): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) { if (await probe()) return; await new Promise((resolve) => setTimeout(resolve, 20)); }
  throw new Error("Desktop recovery scenario timed out.");
}
