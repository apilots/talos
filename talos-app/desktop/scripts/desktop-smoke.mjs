import { spawn } from "node:child_process";
import { existsSync, mkdirSync, writeFileSync } from "node:fs";
import net from "node:net";
import path from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const desktopDir = path.resolve(scriptDir, "..");
const repoRoot = path.resolve(desktopDir, "../..");
const artifactDir = path.resolve(repoRoot, ".tmp/desktop-smoke", timestamp());
const explicitAppServerBin = process.env.TALOS_APP_SERVER_BIN;
const appServerBin = explicitAppServerBin ?? path.resolve(repoRoot, "talos-rs/target/debug/talos");
const launchToken = process.env.TALOS_DESKTOP_LAUNCH_TOKEN ?? "desktop-smoke-token";
const appExecutable = process.env.TALOS_DESKTOP_SMOKE_APP;
const smokeUsername = process.env.TALOS_DESKTOP_SMOKE_USERNAME ?? "admin";
const smokePassword = process.env.TALOS_DESKTOP_SMOKE_PASSWORD ?? "admin@123";
const smokePrompt = `Talos desktop smoke ${timestamp()} - reply exactly ok.`;
const createdSessionIds = [];
let cdp;

if (!appExecutable && !existsSync(appServerBin)) {
  throw new Error(`Missing app-server binary: ${appServerBin}. Run: cargo build -p talos`);
}

mkdirSync(artifactDir, { recursive: true });

const debugPort = await findAvailablePort(Number(process.env.TALOS_DESKTOP_DEBUG_PORT ?? 9223));
const appCommand = appExecutable ?? "electron";
const appArgs = appExecutable
  ? [`--remote-debugging-port=${debugPort}`]
  : [desktopDir, `--remote-debugging-port=${debugPort}`];
const childEnv = {
  ...process.env,
  BAILIAN_CODINGPLAN_API_KEY: process.env.BAILIAN_CODINGPLAN_API_KEY ?? "desktop-smoke-dummy",
  JWT_SECRET: process.env.JWT_SECRET ?? "desktop-smoke-jwt-secret",
  TALOS_DESKTOP_LAUNCH_TOKEN: launchToken,
  TALOS_LOCAL_LAUNCH_TOKEN: launchToken,
};
if (!appExecutable || explicitAppServerBin) {
  childEnv.TALOS_APP_SERVER_BIN = appServerBin;
}

const electron = spawn(appCommand, appArgs, {
  cwd: appExecutable ? path.dirname(appExecutable) : desktopDir,
  env: childEnv,
  stdio: ["ignore", "pipe", "pipe"],
});

const logs = [];
electron.stdout.on("data", (chunk) => logs.push(chunk.toString("utf8")));
electron.stderr.on("data", (chunk) => logs.push(chunk.toString("utf8")));

try {
  const page = await waitForPage(debugPort, 30_000);
  cdp = await connectCdp(page.webSocketDebuggerUrl);

  await cdp.call("Runtime.enable");
  await cdp.call("Page.enable");
  await waitForExpression(cdp, "Boolean(window.talos)", 30_000);
  await waitForExpression(cdp, "document.title === 'Talos'", 30_000);
  const serverInfo = await evaluate(cdp, "window.talos.connect().then((response) => response.server_info)");

  if (serverInfo?.auth_enabled === true) {
    await waitForExpression(
      cdp,
      "Boolean(document.querySelector('#auth-username')) || Boolean(document.querySelector('[data-smoke=\"chat-composer-input\"]'))",
      30_000,
    );
    const loginVisible = await evaluate(cdp, "Boolean(document.querySelector('#auth-username'))");
    if (loginVisible) {
      await setInputValue(cdp, "#auth-username", smokeUsername);
      await setInputValue(cdp, "#auth-password", smokePassword);
      await evaluate(cdp, "document.querySelector('form button[type=\"submit\"]')?.click(); undefined");
    }
    await waitForExpression(cdp, "Boolean(document.querySelector('[data-smoke=\"chat-composer-input\"]'))", 30_000);
  }

  await evaluate(
    cdp,
    `(() => {
      const raw = window.localStorage?.getItem("talos-web-ui");
      const token = raw ? JSON.parse(raw)?.state?.connection?.authToken : "";
      window.__talosSmokeActorCredentials = token
        ? { credential: { type: "existing_session_token", token } }
        : null;
      window.__talosSmokeInvoke = (method, params) => window.talos.invoke(
        method,
        params,
        window.__talosSmokeActorCredentials
          ? { actorCredentials: window.__talosSmokeActorCredentials }
          : undefined
      );
      return true;
    })()`,
  );

  const modelCount = await waitForEvaluate(
    cdp,
    "window.__talosSmokeInvoke('model.list', {}).then((r) => r.models?.length ?? r.data?.length ?? 0)",
    30_000,
  );
  const toolCount = await waitForEvaluate(
    cdp,
    "window.__talosSmokeInvoke('tool.list', {}).then((r) => r.tools?.length ?? r.data?.length ?? 0)",
    30_000,
  );
  const account = await waitForEvaluate(
    cdp,
    serverInfo?.auth_enabled === true
      ? "window.__talosSmokeInvoke('user.current', {})"
      : "window.__talosSmokeInvoke('account.get', {})",
    30_000,
  );
  const skillCount = await waitForEvaluate(
    cdp,
    "window.__talosSmokeInvoke('skill.list', {}).then((r) => r.skills?.length ?? r.data?.length ?? 0)",
    30_000,
  );
  const appCount = await waitForEvaluate(
    cdp,
    "window.__talosSmokeInvoke('app.list', {}).then((r) => r.apps?.length ?? r.data?.length ?? 0)",
    30_000,
  );
  const adminVisible = await evaluate(
    cdp,
    "Array.from(document.querySelectorAll('a')).some((a) => (a.textContent || '').toLowerCase().includes('admin') || (a.getAttribute('href') || '').includes('/admin'))",
  );

  assert(Number.isInteger(modelCount) && modelCount >= 0, "model.list did not return a model count.");
  assert(toolCount > 0, "tool.list returned no tools.");
  assert(skillCount > 0, "skill.list returned no discoverable skills.");
  assert(appCount > 0, "app.list returned no app shells.");
  assert(typeof serverInfo?.auth_enabled === "boolean", `desktop server info did not include auth state: ${JSON.stringify(serverInfo)}`);
  if (serverInfo.auth_enabled) {
    assert(account?.username === smokeUsername, `desktop login did not return the smoke user: ${JSON.stringify(account)}`);
    assert(adminVisible === (account?.role === "admin"), "Admin navigation should match authenticated user role.");
  } else {
    assert(account?.role === "admin", `account.get did not reflect open-mode desktop admin access: ${JSON.stringify(account)}`);
    assert(adminVisible === false, "Admin navigation should stay hidden in open auth mode.");
  }

  await evaluate(cdp, "location.hash = '#/'; undefined");
  await waitForExpression(cdp, "Boolean(document.querySelector('[data-smoke=\"chat-composer-input\"]'))", 30_000);
  await setInputValue(cdp, "[data-smoke=\"chat-composer-input\"]", smokePrompt);
  await evaluate(cdp, "document.querySelector('[data-smoke=\"chat-composer-send\"]')?.click(); undefined");
  await waitForExpression(cdp, "location.hash.startsWith('#/sessions/')", 30_000);
  const createdSessionHash = await evaluate(cdp, "location.hash");
  assert(createdSessionHash.startsWith("#/sessions/"), `Home composer did not navigate to a session: ${createdSessionHash}`);
  const createdSessionId = createdSessionHash.replace(/^#\/sessions\//, "");
  createdSessionIds.push(createdSessionId);
  await waitForExpression(cdp, `(document.body.textContent || '').includes(${JSON.stringify(smokePrompt)})`, 30_000);

  const routeInteractions = [];
  const composerStopState = await readComposerStopState(cdp);
  assert(
    composerStopState.state !== "disabled",
    `Composer stop button rendered disabled while streaming: ${JSON.stringify(composerStopState)}`,
  );
  await clickAnchorByHash(cdp, "#/projects");
  await waitForHash(cdp, "#/projects", 30_000);
  await sleep(1_500);
  const navigationHashAfterRunningTurn = await evaluate(cdp, "location.hash");
  assert(
    navigationHashAfterRunningTurn === "#/projects",
    `Running session redirected after route click: ${navigationHashAfterRunningTurn}`,
  );
  routeInteractions.push({ name: "projects-during-turn", route: "#/projects", method: "click" });

  await waitForExpression(
    cdp,
    `window.__talosSmokeInvoke("session.turns", { session_id: ${JSON.stringify(createdSessionId)} }).then((response) => {
      const turns = response?.data ?? [];
      return turns.some((turn) => Array.isArray(turn.output) && turn.output.some((item) => item.type === "user_input" && item.text === ${JSON.stringify(smokePrompt)}));
    })`,
    120_000,
  );

  const linkedRoutes = [
    ["home", "#/"],
    ["projects", "#/projects"],
    ["files", "#/files"],
  ];

  for (const [name, route] of linkedRoutes) {
    await clickAnchorByHash(cdp, route);
    await waitForHash(cdp, route, 30_000);
    await captureScreenshot(cdp, path.join(artifactDir, `${name}.png`));
    routeInteractions.push({ name, route, method: "click" });
  }

  await clickAccountMenuSettings(cdp);
  await waitForHash(cdp, "#/settings", 30_000);
  await captureScreenshot(cdp, path.join(artifactDir, "settings.png"));
  routeInteractions.push({ name: "settings", route: "#/settings", method: "click" });

  const settingsRoutes = [
    ["settings-models", "#/settings/models"],
    ["settings-agent-workspace", "#/settings/agent-workspace"],
    ["settings-permissions", "#/settings/permissions"],
    ["settings-appearance", "#/settings/appearance"],
    ["settings-advanced", "#/settings/advanced"],
  ];
  if (serverInfo.auth_enabled === true) {
    settingsRoutes.splice(3, 0, ["settings-account", "#/settings/account"]);
  }

  for (const [name, route] of settingsRoutes) {
    await clickAnchorByHash(cdp, route);
    await waitForHash(cdp, route, 30_000);
    await captureScreenshot(cdp, path.join(artifactDir, `${name}.png`));
    routeInteractions.push({ name, route, method: "click" });
  }

  const directRoutes = [
    ["memory", "#/memory"],
    ["workspace-docs", "#/workspace-docs"],
  ];
  for (const [name, route] of directRoutes) {
    await evaluate(cdp, `location.hash = ${JSON.stringify(route)}; undefined`);
    await waitForHash(cdp, route, 30_000);
    await captureScreenshot(cdp, path.join(artifactDir, `${name}.png`));
    routeInteractions.push({ name, route, method: "direct-hash" });
  }

  await evaluate(cdp, "location.hash = '#/settings/advanced'; undefined");
  await waitForExpression(cdp, "location.hash === '#/settings/advanced'", 30_000);
  await waitForExpression(cdp, "(document.body.textContent || '').includes('Desktop local runtime')", 30_000);

  await clickAnchorByHash(cdp, "#/settings/models");
  await waitForHash(cdp, "#/settings/models", 30_000);
  await waitForExpression(
    cdp,
    `(() => {
      const text = document.body.textContent || "";
      return !text.includes("Loading configuration") && !text.includes("加载配置中");
    })()`,
    30_000,
  );
  await waitForExpression(
    cdp,
    `(() => {
      const text = document.body.textContent || "";
      return (text.includes("Models") || text.includes("模型")) &&
        (text.includes("Active Provider") ||
          text.includes("当前 Provider") ||
          ["Anthropic", "OpenAI", "Bailian", "Ollama"].every((label) => text.includes(label)));
    })()`,
    30_000,
  );

  await clickAdminIfAvailable(cdp);
  if (account?.role === "admin" && serverInfo.auth_enabled) {
    await waitForHash(cdp, "#/admin", 30_000);
    await captureScreenshot(cdp, path.join(artifactDir, "admin.png"));
    routeInteractions.push({ name: "admin", route: "#/admin", method: "click" });
  } else {
    await waitForExpression(cdp, "location.hash === '#/'", 30_000);
    routeInteractions.push({ name: "admin-guard", route: "#/", method: "click-or-redirect" });
  }

  const cleanup = await cleanupCreatedSessions(cdp, createdSessionIds);
  assert(
    cleanup.every((entry) => entry.deleted),
    `Desktop smoke failed to clean created sessions: ${JSON.stringify(cleanup)}`,
  );
  const report = {
    status: "passed",
    artifactDir,
    appCommand,
    appServerBin: appExecutable && !explicitAppServerBin ? "bundled" : appServerBin,
    debugPort,
    account,
    serverInfo,
    modelCount,
    toolCount,
    skillCount,
    appCount,
    composerStopState,
    createdSessionIds,
    cleanup,
    routes: routeInteractions,
  };
  writeFileSync(path.join(artifactDir, "desktop-smoke-report.json"), `${JSON.stringify(report, null, 2)}\n`);
  console.log(`Desktop smoke passed: ${artifactDir}`);
  cdp.close();
  cdp = undefined;
} catch (error) {
  if (cdp) {
    try {
      const cleanup = await cleanupCreatedSessions(cdp, createdSessionIds, { cancelFirst: true });
      logs.push(`\nCleanup after failure: ${JSON.stringify(cleanup)}\n`);
    } catch (cleanupError) {
      logs.push(`\nCleanup after failure failed: ${errorMessage(cleanupError)}\n`);
    }
  }
  writeFileSync(path.join(artifactDir, "desktop-smoke-error.log"), `${errorMessage(error)}\n\n${logs.join("")}`);
  throw error;
} finally {
  electron.kill("SIGTERM");
  await sleep(500);
  if (electron.exitCode === null) {
    electron.kill("SIGKILL");
  }
  cdp?.close?.();
}

async function findAvailablePort(start) {
  for (let port = start; port < start + 100; port += 1) {
    if (await isPortFree(port)) {
      return port;
    }
  }
  throw new Error(`No available debug port found from ${start}.`);
}

function isPortFree(port) {
  return new Promise((resolve) => {
    const server = net.createServer();
    server.once("error", () => resolve(false));
    server.once("listening", () => {
      server.close(() => resolve(true));
    });
    server.listen(port, "127.0.0.1");
  });
}

async function waitForPage(port, timeoutMs) {
  const startedAt = Date.now();
  while (Date.now() - startedAt < timeoutMs) {
    try {
      const response = await fetch(`http://127.0.0.1:${port}/json/list`);
      const pages = await response.json();
      const page = pages.find((candidate) => candidate.type === "page" && candidate.webSocketDebuggerUrl);
      if (page) {
        return page;
      }
    } catch {
      // Electron may not have opened the debugging endpoint yet.
    }
    await sleep(200);
  }
  throw new Error(`Timed out waiting for Electron debugging page on port ${port}.`);
}

function connectCdp(url) {
  const socket = new WebSocket(url);
  let nextId = 1;
  const pending = new Map();

  socket.addEventListener("message", (event) => {
    const message = JSON.parse(event.data);
    if (!message.id) {
      return;
    }
    const request = pending.get(message.id);
    if (!request) {
      return;
    }
    pending.delete(message.id);
    if (message.error) {
      request.reject(new Error(message.error.message));
      return;
    }
    request.resolve(message.result);
  });

  return new Promise((resolve, reject) => {
    socket.addEventListener("open", () => {
      resolve({
        call(method, params = {}) {
          const id = nextId++;
          socket.send(JSON.stringify({ id, method, params }));
          return new Promise((callResolve, callReject) => {
            pending.set(id, { resolve: callResolve, reject: callReject });
          });
        },
        close() {
          socket.close();
        },
      });
    }, { once: true });
    socket.addEventListener("error", () => reject(new Error("Failed to connect to Electron CDP.")), { once: true });
  });
}

async function waitForExpression(cdp, expression, timeoutMs) {
  const startedAt = Date.now();
  while (Date.now() - startedAt < timeoutMs) {
    if (await evaluate(cdp, expression)) {
      return;
    }
    await sleep(200);
  }
  throw new Error(`Timed out waiting for expression: ${expression}`);
}

async function waitForEvaluate(cdp, expression, timeoutMs) {
  const startedAt = Date.now();
  let lastError;
  while (Date.now() - startedAt < timeoutMs) {
    try {
      return await evaluate(cdp, expression);
    } catch (error) {
      lastError = error;
      await sleep(300);
    }
  }
  throw new Error(`Timed out evaluating expression: ${expression}\n${errorMessage(lastError)}`);
}

async function evaluate(cdp, expression) {
  const result = await cdp.call("Runtime.evaluate", {
    expression,
    awaitPromise: true,
    returnByValue: true,
  });
  if (result.exceptionDetails) {
    throw new Error(result.exceptionDetails.text ?? "Runtime.evaluate failed.");
  }
  return result.result.value;
}

async function captureScreenshot(cdp, outputPath) {
  const result = await cdp.call("Page.captureScreenshot", { format: "png", captureBeyondViewport: true });
  writeFileSync(outputPath, Buffer.from(result.data, "base64"));
}

async function setInputValue(cdp, selector, value) {
  await evaluate(
    cdp,
    `(() => {
      const element = document.querySelector(${JSON.stringify(selector)});
      if (!element) return false;
      const setter = element instanceof HTMLTextAreaElement
        ? Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")?.set
        : Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")?.set;
      setter?.call(element, ${JSON.stringify(value)});
      element.dispatchEvent(new Event("input", { bubbles: true }));
      return true;
    })()`,
  );
}

async function clickAnchorByHash(cdp, hash) {
  const clicked = await evaluate(
    cdp,
    `(() => {
      const target = ${JSON.stringify(hash)};
      const anchor = Array.from(document.querySelectorAll("a")).find((candidate) => {
        const href = candidate.getAttribute("href") || "";
        return candidate.hash === target || href === target || href.endsWith(target);
      });
      if (!anchor) return false;
      anchor.click();
      return true;
    })()`,
  );
  assert(clicked, `Anchor not found for hash: ${hash}`);
}

async function clickAccountMenuSettings(cdp) {
  const opened = await evaluate(
    cdp,
    `(() => {
      const button = document.querySelector("button[aria-expanded]");
      if (!button) return false;
      button.click();
      return true;
    })()`,
  );
  assert(opened, "Account menu button not found.");
  await waitForExpression(
    cdp,
    "Array.from(document.querySelectorAll('a')).some((anchor) => anchor.hash === '#/settings' || (anchor.getAttribute('href') || '').endsWith('#/settings'))",
    30_000,
  );
  await clickAnchorByHash(cdp, "#/settings");
}

async function clickAdminIfAvailable(cdp) {
  const result = await evaluate(
    cdp,
    `(() => {
      const anchor = Array.from(document.querySelectorAll("a")).find((candidate) => {
        const href = candidate.getAttribute("href") || "";
        return candidate.hash === "#/admin" || href === "#/admin" || href.endsWith("#/admin");
      });
      if (anchor) {
        anchor.click();
        return "clicked";
      }
      location.hash = "#/";
      return "unavailable";
    })()`,
  );
  assert(result === "clicked" || result === "unavailable", `Unexpected admin click result: ${result}`);
  return result;
}

async function readComposerStopState(cdp) {
  return evaluate(
    cdp,
    `(() => {
      const button = document.querySelector('[data-smoke="chat-composer-send"]');
      if (!button) return { state: "missing" };
      const label = button.getAttribute("aria-label") || button.getAttribute("title") || "";
      const busy = label.includes("Stop response") || label.includes("停止回答");
      const disabled = Boolean(button.disabled);
      if (!busy) return { state: "idle", label, disabled };
      return { state: disabled ? "disabled" : "clickable", label, disabled };
    })()`,
  );
}

function waitForHash(cdp, hash, timeoutMs) {
  return waitForExpression(cdp, `location.hash === ${JSON.stringify(hash)}`, timeoutMs);
}

async function cleanupCreatedSessions(cdp, sessionIds, options = {}) {
  const uniqueSessionIds = [...new Set(sessionIds.filter(Boolean))];
  const results = [];
  for (const sessionId of uniqueSessionIds) {
    const result = await evaluate(
      cdp,
      `(() => window.__talosSmokeInvoke("session.delete", { session_id: ${JSON.stringify(sessionId)} })
        .then(() => ({ session_id: ${JSON.stringify(sessionId)}, deleted: true }))
        .catch(async (deleteError) => {
          if (${JSON.stringify(Boolean(options.cancelFirst))}) {
            await window.__talosSmokeInvoke("turn.interrupt", { session_id: ${JSON.stringify(sessionId)} }).catch(() => undefined);
            await new Promise((resolve) => setTimeout(resolve, 500));
            return window.__talosSmokeInvoke("session.delete", { session_id: ${JSON.stringify(sessionId)} })
              .then(() => ({ session_id: ${JSON.stringify(sessionId)}, deleted: true, cancelled_first: true }))
              .catch((retryError) => ({
                session_id: ${JSON.stringify(sessionId)},
                deleted: false,
                error: retryError instanceof Error ? retryError.message : String(retryError),
                first_error: deleteError instanceof Error ? deleteError.message : String(deleteError),
              }));
          }
          return {
            session_id: ${JSON.stringify(sessionId)},
            deleted: false,
            error: deleteError instanceof Error ? deleteError.message : String(deleteError),
          };
        }))()`,
    );
    results.push(result);
  }
  return results;
}

function assert(condition, message) {
  if (!condition) {
    throw new Error(message);
  }
}

function sleep(ms) {
  return new Promise((resolve) => {
    setTimeout(resolve, ms);
  });
}

function timestamp() {
  return new Date().toISOString().replace(/[-:]/g, "").replace(/\..+$/, "").replace("T", "_");
}

function errorMessage(error) {
  return error instanceof Error ? error.stack ?? error.message : String(error);
}
