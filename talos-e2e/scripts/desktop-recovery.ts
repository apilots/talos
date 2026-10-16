/** Secretless public regression: real stdio authentication and backend replacement. */
import assert from "node:assert/strict";
import { createServer } from "node:http";
import { createServer as createSocket } from "node:net";
import { spawn, execFileSync } from "node:child_process";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import { randomUUID } from "node:crypto";
import { AppProtocol, TalosApiError } from "../../talos-app/ts-sdk/packages/core/src/index";
import { createDesktopMainAppServerBridge, type DesktopMainAppServerBridge } from "../../talos-app/ts-sdk/packages/app-server/src/desktop-main";
import { DesktopAppServerController } from "../../talos-app/desktop/src/app-server-controller";

const home = process.env.TALOS_HOME!;
const user = { user_id: "recovery-owner", username: "recovery", email: "recovery@example.invalid", role: "user", type: "free", disabled: false, created_at: 1, updated_at: 1 };
const token = randomUUID();
let introspections = 0;
let completions = 0;
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
    response.end(`data: ${JSON.stringify(chunk)}\n\ndata: ${JSON.stringify(end)}\n\ndata: [DONE]\n\n`);
    return;
  }
  if (request.url === "/v1/me") {
    if (request.headers.authorization !== `Bearer ${token}`) { response.writeHead(401).end(); return; }
    response.setHeader("content-type", "application/json");
    response.end(JSON.stringify({ ...user, status: "active", email_verified: true, set_password_required: false, created_at_ms: 1, updated_at_ms: 1 }));
    return;
  }
  if (request.url !== "/v1/auth/introspect") { response.writeHead(404).end(); return; }
  introspections += 1;
  response.setHeader("content-type", "application/json");
  response.end(JSON.stringify({ active: JSON.parse(body).token === token,
    user: { ...user, status: "active" }, capabilities: { is_admin: false, role: "user", scopes: ["sessions:read", "sessions:write"] },
    entitlement: { can_use: true, plan: "free", reason: null }, expires_at_ms: null }));
});
await new Promise<void>((resolve) => auth.listen(0, "127.0.0.1", resolve));
let controllerToClose: DesktopAppServerController | undefined;
let serverToClose: ReturnType<typeof spawn> | undefined;
try {
  const address = auth.address();
  assert(address && typeof address !== "string");
  const api = `http://127.0.0.1:${address.port}`;
  const configPath = process.env.TALOS_CONFIG!;
  const config = JSON.parse(await readFile(configPath, "utf8"));
  config.gateway.auth = true;
  config.cloud.api_base_url = api;
  config.sync.enabled = false;
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
  await writeFile(configPath, JSON.stringify(config));
  const serverHome = path.join(home, "server");
  await mkdir(path.join(serverHome, "conf"), { recursive: true });
  const serverConfig = structuredClone(config);
  serverConfig.gateway.port = httpPort;
  serverConfig.agent_node = { control_plane: { bind_address: `127.0.0.1:${rpcPort}`, tls_certificate_file: cert, tls_private_key_file: key, trusted_node_ca_file: cert } };
  const serverPath = path.join(serverHome, "conf/talos.yaml");
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
    command: process.env.TALOS_APP_SERVER_BIN, env: { ...process.env, TALOS_DESKTOP_HOSTNAME: "recovery-desktop" }, desktopHost: true,
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
  const options = { actorCredentials: { credential: { type: "existing_session_token" as const, token } } };
  const params = { sessionId: randomUUID(), turnId: null, sinceEventId: null, replayLimit: 50, closeWhenIdle: false, closeOnTerminal: false };
  await controller.invoke("sync/status", null, options);
  const turn = { sessionId: params.sessionId, input: { content: [{ type: "text", text: "before restart" }] }, modelSelection: null, userPermissionMode: "fullAccess", projectId: null, currentDirectory: null, clientRequestId: randomUUID() };
  await controller.invoke("turn/start", turn, options);
  await waitFor(async () => (await controller.invoke<AppProtocol.SessionHistoryResponse>("session/history", { sessionId: params.sessionId, limit: 50, cursor: null }, options)).data.length === 1);
  await assert.rejects(bridges[0]!.invoke("event/subscribe", params), (error) => error instanceof TalosApiError && error.code === "unauthenticated");
  const subscribed = await controller.invoke<AppProtocol.SubscribeTimelineResponse>("event/subscribe", params, options);
  await waitFor(async () => events.some((event) => event.event.type === "agent" && event.event.data.payload.type === "turnCompleted"));
  const cursor = events.at(-1)!.eventId;
  const identity = await controller.invoke<AppProtocol.DesktopWorkerIdentityResponse>("desktop/workerIdentity", null);
  const beforeIds = events.map((event) => event.eventId);
  assert(bridges[0]!.client.transport.terminate("SIGTERM"));
  const deadline = Date.now() + 30000;
  while (bridges.length < 2 && Date.now() < deadline) {
    await new Promise((resolve) => setTimeout(resolve, 20));
  }
  await controller.invoke("user/current", {}, options);
  assert.deepEqual(await controller.invoke("desktop/workerIdentity", null), identity);
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
  assert(introspections > 0);
  console.log("CASE desktop_authenticated_backend_restart PASS");
} finally {
  try { await controllerToClose?.close(); }
  finally {
    if (serverToClose && serverToClose.exitCode === null && serverToClose.signalCode === null) {
      const exited = new Promise<void>((resolve) => serverToClose!.once("exit", () => resolve()));
      serverToClose.kill("SIGINT");
      await exited;
    }
    await new Promise<void>((resolve, reject) => auth.close((error) => error ? reject(error) : resolve()));
  }
}

async function freePort(): Promise<number> {
  const socket = createSocket();
  await new Promise<void>((resolve) => socket.listen(0, "127.0.0.1", resolve));
  const address = socket.address();
  assert(address && typeof address !== "string");
  await new Promise<void>((resolve) => socket.close(() => resolve()));
  return address.port;
}
async function waitFor(probe: () => Promise<boolean>): Promise<void> {
  const deadline = Date.now() + 30000;
  while (Date.now() < deadline) { if (await probe()) return; await new Promise((resolve) => setTimeout(resolve, 20)); }
  throw new Error("Desktop recovery scenario timed out.");
}
