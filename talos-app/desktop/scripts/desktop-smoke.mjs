import { spawn } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync, writeFileSync } from "node:fs";
import net from "node:net";
import path from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const desktopDir = path.resolve(scriptDir, "..");
const repoRoot = path.resolve(desktopDir, "../..");
const artifactDir = path.resolve(repoRoot, ".tmp/desktop-smoke", timestamp());
const smokeHome = path.join(artifactDir, "home");
const talosHome = path.join(smokeHome, ".talos");
const talosConfig = path.join(repoRoot, "conf/talos-dev.yaml");
const explicitAppServerBin = process.env.TALOS_APP_SERVER_BIN;
const appServerBin = explicitAppServerBin ?? path.resolve(repoRoot, "talos-rs/target/debug/app-server");
const appExecutable = process.env.TALOS_DESKTOP_SMOKE_APP;
let cdp;

if (!appExecutable && !existsSync(appServerBin)) {
  throw new Error(`Missing app-server binary: ${appServerBin}. Run: cargo build -p app-server --bin app-server`);
}

mkdirSync(artifactDir, { recursive: true });
configureSmokeHome();

const debugPort = await findAvailablePort(Number(process.env.TALOS_DESKTOP_DEBUG_PORT ?? 9223));
const appCommand = appExecutable ?? "electron";
const appArgs = appExecutable
  ? [`--remote-debugging-port=${debugPort}`]
  : [desktopDir, `--remote-debugging-port=${debugPort}`];
const childEnv = {
  ...process.env,
  HOME: smokeHome,
  TALOS_HOME: talosHome,
  TALOS_CONFIG: talosConfig,
  TALOS_LLM_ANTHROPIC_API_KEY: process.env.TALOS_LLM_ANTHROPIC_API_KEY ?? "desktop-smoke-dummy",
};
if (!appExecutable || explicitAppServerBin) {
  childEnv.TALOS_APP_SERVER_BIN = appServerBin;
}

function configureSmokeHome() {
  const confRoot = path.join(talosHome, "conf");
  const skillRoot = path.join(smokeHome, ".agents", "skills", "desktop-smoke");
  mkdirSync(confRoot, { recursive: true });
  mkdirSync(skillRoot, { recursive: true });
  copyFileSync(talosConfig, path.join(confRoot, "talos.yaml"));
  writeFileSync(
    path.join(skillRoot, "SKILL.md"),
    `---
name: desktop-smoke
description: Desktop smoke skill.
---

Desktop smoke skill.
`,
  );
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
      "Array.from(document.querySelectorAll('button')).some((button) => (button.textContent || '').includes('Talos Cloud'))",
      30_000,
    );
    throw new Error("Desktop smoke does not automate Talos Cloud browser login. Run this smoke with gateway.auth=false or validate auth manually.");
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
  const account = await waitForEvaluate(
    cdp,
    "window.__talosSmokeInvoke('user.current', {})",
    30_000,
  );
  const skillCount = await waitForEvaluate(
    cdp,
    "window.__talosSmokeInvoke('skills.list', { include_disabled: false }).then((r) => r.data?.length ?? 0)",
    30_000,
  );
  const skillSuggestionCount = await waitForEvaluate(
    cdp,
    "window.__talosSmokeInvoke('skills.suggest', { query: '', limit: 5 }).then((r) => r.data?.length ?? 0)",
    30_000,
  );
  const adminVisible = await evaluate(
    cdp,
    "Array.from(document.querySelectorAll('a')).some((a) => (a.textContent || '').toLowerCase().includes('admin') || (a.getAttribute('href') || '').includes('/admin'))",
  );

  assert(Number.isInteger(modelCount) && modelCount >= 0, "model.list did not return a model count.");
  assert(skillCount > 0, "skills.list returned no discoverable skills.");
  assert(skillSuggestionCount > 0, "skills.suggest returned no discoverable skills.");
  assert(typeof serverInfo?.auth_enabled === "boolean", `desktop server info did not include auth state: ${JSON.stringify(serverInfo)}`);
  assert(account?.user?.role === "user", `user.current did not reflect open-mode anonymous user role: ${JSON.stringify(account)}`);
  assert(account?.user?.type === "free", `user.current did not reflect open-mode anonymous user type: ${JSON.stringify(account)}`);
  assert(adminVisible === false, "Admin navigation should not be rendered in Talos.");

  await evaluate(cdp, "location.hash = '#/'; undefined");
  await waitForExpression(cdp, "Boolean(document.querySelector('[data-smoke=\"chat-composer-input\"]'))", 30_000);
  await captureScreenshot(cdp, path.join(artifactDir, "home.png"));

  const routeInteractions = [];
  const linkedRoutes = [
    ["plugins", "#/plugins"],
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
    ["settings-server", "#/settings/server"],
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
  ];
  for (const [name, route] of directRoutes) {
    await evaluate(cdp, `location.hash = ${JSON.stringify(route)}; undefined`);
    await waitForHash(cdp, route, 30_000);
    await captureScreenshot(cdp, path.join(artifactDir, `${name}.png`));
    routeInteractions.push({ name, route, method: "direct-hash" });
  }

  await evaluate(cdp, "location.hash = '#/settings/server'; undefined");
  await waitForExpression(cdp, "location.hash === '#/settings/server'", 30_000);
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

  const report = {
    status: "passed",
    artifactDir,
    appCommand,
    appServerBin: appExecutable && !explicitAppServerBin ? "bundled" : appServerBin,
    debugPort,
    account,
    serverInfo,
    modelCount,
    skillCount,
    routes: routeInteractions,
  };
  writeFileSync(path.join(artifactDir, "desktop-smoke-report.json"), `${JSON.stringify(report, null, 2)}\n`);
  console.log(`Desktop smoke passed: ${artifactDir}`);
  cdp.close();
  cdp = undefined;
} catch (error) {
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

function waitForHash(cdp, hash, timeoutMs) {
  return waitForExpression(cdp, `location.hash === ${JSON.stringify(hash)}`, timeoutMs);
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
