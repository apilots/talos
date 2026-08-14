import { spawn } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
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
const modeSmokeFiles = [1, 2, 3].map((index) =>
  path.join(talosHome, "workspace-talos", `desktop-mode-smoke-${index}.txt`)
);
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
  const workspaceRoot = path.join(talosHome, "workspace-talos");
  const skillRoot = path.join(smokeHome, ".agents", "skills", "desktop-smoke");
  mkdirSync(confRoot, { recursive: true });
  mkdirSync(workspaceRoot, { recursive: true });
  mkdirSync(skillRoot, { recursive: true });
  copyFileSync(talosConfig, path.join(confRoot, "talos.yaml"));
  modeSmokeFiles.forEach((file, index) => {
    writeFileSync(file, `DESKTOP_MODE_CONTENT_${index + 1}\n`);
  });
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

  await installSmokeInvoke(cdp);

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

  const interaction = process.env.TALOS_DESKTOP_SMOKE_INTERACTION === "1"
    ? await runInteractionSmoke(cdp, artifactDir)
    : null;

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
    ["settings-appearance", "#/settings/appearance"],
    ["settings-server", "#/settings/server"],
  ];
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
    interaction,
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

async function runInteractionSmoke(cdp, outputDir) {
  await sendComposerMessage(cdp, "Reply with exactly DESKTOP_INTERACTION_READY");
  await waitForExpression(cdp, "location.hash.startsWith('#/sessions/')", 30_000);
  const sessionId = await evaluate(cdp, "location.hash.split('/').pop()");
  assert(sessionId, "Desktop interaction did not create a session.");
  await waitForActiveTurnStatus(cdp, sessionId, "completed", 60_000);
  await evaluate(
    cdp,
    `history.replaceState(null, document.title, ${JSON.stringify(`#/sessions/${sessionId}`)}); location.reload(); undefined`,
  );
  await waitForExpression(cdp, "Boolean(window.talos)", 30_000);
  await installSmokeInvoke(cdp);
  await waitForHash(cdp, `#/sessions/${sessionId}`, 30_000);
  await sleep(1_000);
  await captureScreenshot(cdp, path.join(outputDir, "interaction-reloaded.png"));
  const reloadState = await evaluate(
    cdp,
    `(() => {
      const button = document.querySelector('[data-smoke="chat-composer-send"]');
      return {
        bodyText: (document.body.textContent || '').slice(0, 2_000),
        button: button ? { disabled: button.disabled, ariaLabel: button.getAttribute('aria-label') } : null,
        hash: location.hash,
      };
    })()`,
  );
  writeFileSync(path.join(outputDir, "interaction-reloaded.json"), `${JSON.stringify(reloadState, null, 2)}\n`);
  await waitForComposerIdle(cdp, 60_000);
  const session = await waitForEvaluate(
    cdp,
    `window.__talosSmokeInvoke('session.get', { session_id: ${JSON.stringify(sessionId)} }).then((response) => {
      if (!response.session) throw new Error('session not available');
      return response.session;
    })`,
    30_000,
  );
  const permissionModes = [
    ["askForApproval", "ask-for-approval", "Ask for approval"],
    ["approveForMe", "approve-for-me", "Approve for me"],
    ["fullAccess", "full-access", "Full access"],
  ];
  const modeResults = [];
  for (const [mode, screenshotName, label] of permissionModes) {
    await evaluate(
      cdp,
      `window.__talosSmokeInvoke('session.patch', {
        session_id: ${JSON.stringify(sessionId)},
        conf: { modelSelection: null, userPermissionMode: ${JSON.stringify(mode)} },
        project_id: null,
        current_directory: null
      }).then((response) => {
        if (response.session?.effectiveMode !== ${JSON.stringify(mode)}) {
          throw new Error('session permission mode was not updated');
        }
        return response.session;
      })`,
    );
    await evaluate(cdp, "location.reload(); undefined");
    await waitForExpression(cdp, "Boolean(window.talos)", 30_000);
    await installSmokeInvoke(cdp);
    await waitForHash(cdp, `#/sessions/${sessionId}`, 30_000);
    await waitForComposerIdle(cdp, 60_000);
    await waitForExpression(
      cdp,
      `(document.body.textContent || '').includes(${JSON.stringify(label)})`,
      30_000,
    );

    const marker = `DESKTOP_MODE_${mode}_OK`;
    await sendComposerMessage(
      cdp,
      `You must make exactly three independent Read tool calls, one for each exact path in this order: ${modeSmokeFiles.join(", ")}. Do not call any other tool. After all three tool results, respond with exactly ${marker}`,
    );
    await waitForActiveTurnStatus(cdp, sessionId, "completed", 90_000);
    await waitForComposerIdle(cdp, 60_000);
    const modeHistory = await evaluate(
      cdp,
      `window.__talosSmokeInvoke('session.history', { session_id: ${JSON.stringify(sessionId)} }).then((response) => {
        const message = response.history.messages.at(-1);
        const toolCalls = message?.items.filter((item) => item.content.type === 'tool_call') || [];
        return {
          stopReason: message?.stop_reason,
          toolCalls: toolCalls.map((item) => ({
            name: item.content.name,
            status: item.status,
            hasResult: item.content.result !== null
          })),
          metrics: message?.metrics
        };
      })`,
    );
    assert(
      modeHistory?.stopReason === "end_turn",
      `Mode ${mode} did not end: ${JSON.stringify(modeHistory)}`,
    );
    assert(
      modeHistory?.toolCalls?.length === 3,
      `Mode ${mode} did not execute three tools: ${JSON.stringify(modeHistory)}`,
    );
    assert(
      modeHistory.toolCalls.every(
        (call) => call.name === "Read" && call.status === "completed" && call.hasResult
      ),
      `Mode ${mode} tool calls were not terminal: ${JSON.stringify(modeHistory)}`,
    );
    assert(
      modeHistory?.metrics?.tool_calls === 3,
      `Mode ${mode} metrics were incorrect: ${JSON.stringify(modeHistory)}`,
    );
    await evaluate(cdp, "location.reload(); undefined");
    await waitForExpression(cdp, "Boolean(window.talos)", 30_000);
    await installSmokeInvoke(cdp);
    await waitForHash(cdp, `#/sessions/${sessionId}`, 30_000);
    await waitForComposerIdle(cdp, 60_000);
    await waitForExpression(
      cdp,
      `(document.body.textContent || '').includes(${JSON.stringify(marker)})`,
      30_000,
    );
    await sleep(500);
    await captureScreenshot(cdp, path.join(outputDir, `permission-${screenshotName}.png`));
    modeResults.push({ mode, ...modeHistory });
  }

  await sendComposerMessage(
    cdp,
    "You must call AskUserQuestion exactly once. Use header Desktop Choice, question Choose delivery, and exactly these options: id now, label Now, description Deliver now; id later, label Later, description Deliver later. After receiving the answer, respond with exactly DESKTOP_QUESTION_OPTION_OK",
  );
  await waitForExpression(
    cdp,
    "Boolean(document.querySelector('section[data-user-action=\"question\"]')) && (document.body.textContent || '').includes('Desktop Choice')",
    60_000,
  );
  const pendingBeforeRefresh = await waitForActiveTurnStatus(
    cdp,
    sessionId,
    "waiting_user_action",
    30_000,
  );
  const actionIdBeforeRefresh = pendingBeforeRefresh?.pending_user_action?.actionId;
  assert(actionIdBeforeRefresh, "Desktop pending Question did not expose an action id.");
  await evaluate(cdp, "location.reload(); undefined");
  await waitForExpression(cdp, "Boolean(window.talos)", 30_000);
  await installSmokeInvoke(cdp);
  await waitForHash(cdp, `#/sessions/${sessionId}`, 30_000);
  await waitForExpression(
    cdp,
    "Boolean(document.querySelector('section[data-user-action=\"question\"]')) && (document.body.textContent || '').includes('Desktop Choice')",
    60_000,
  );
  const pendingAfterRefresh = await waitForActiveTurnStatus(
    cdp,
    sessionId,
    "waiting_user_action",
    30_000,
  );
  assert(
    pendingAfterRefresh?.pending_user_action?.actionId === actionIdBeforeRefresh,
    `Desktop refresh changed the pending action: ${JSON.stringify({ pendingBeforeRefresh, pendingAfterRefresh })}`,
  );
  await clickButtonInUserAction(cdp, "Now");
  await waitForUserActionClosed(cdp, 60_000);
  await waitForActiveTurnStatus(cdp, sessionId, "completed", 60_000);

  await sendComposerMessage(
    cdp,
    "You must call AskUserQuestion exactly once. Use header Desktop Choice, question Choose delivery, and exactly these options: id now, label Now, description Deliver now; id later, label Later, description Deliver later. After receiving the answer, respond with exactly DESKTOP_QUESTION_OTHER_OK",
  );
  await waitForExpression(
    cdp,
    "Boolean(document.querySelector('section[data-user-action=\"question\"]'))",
    60_000,
  );
  await clickButtonInUserAction(cdp, "Other");
  await setElementValue(cdp, "section[data-user-action=\"question\"] textarea", "Use a staged rollout");
  const submitted = await evaluate(
    cdp,
    `(() => {
      const section = document.querySelector('section[data-user-action="question"]');
      const button = section?.querySelector('button[aria-label]');
      if (!button || button.disabled) return false;
      button.click();
      return true;
    })()`,
  );
  assert(submitted, "Other response submit button was not actionable.");
  await waitForUserActionClosed(cdp, 60_000);
  await waitForActiveTurnStatus(cdp, sessionId, "completed", 60_000);

  const approvalMarker = path.join(talosHome, "workspace-talos", "desktop-approval-marker");
  await evaluate(
    cdp,
    `window.__talosSmokeInvoke('session.patch', {
      session_id: ${JSON.stringify(sessionId)},
      conf: { modelSelection: null, userPermissionMode: 'askForApproval' },
      project_id: null,
      current_directory: null
    }).then((response) => {
      if (response.session?.effectiveMode !== 'askForApproval') {
        throw new Error('session permission mode was not updated');
      }
      return response.session;
    })`,
  );
  await sendComposerMessage(
    cdp,
    `You must call Bash exactly once with this exact command: printf desktop-approval >> '${approvalMarker}'. Do not call any other tool. After the tool result, respond with exactly DESKTOP_APPROVAL_OK`,
  );
  await waitForExpression(
    cdp,
    "Boolean(document.querySelector('section[data-user-action=\"sandbox-approval\"]'))",
    60_000,
  );
  await clickButtonInUserAction(cdp, "Allow once", "sandbox-approval");
  await waitForUserActionClosed(cdp, 60_000);
  await waitForActiveTurnStatus(cdp, sessionId, "completed", 60_000);
  assert(
    readFileSync(approvalMarker, "utf8") === "desktop-approval",
    "Desktop AllowOnce did not execute the approved command exactly once.",
  );
  await evaluate(cdp, "location.reload(); undefined");
  await waitForExpression(cdp, "Boolean(window.talos)", 30_000);
  await installSmokeInvoke(cdp);
  await waitForHash(cdp, `#/sessions/${sessionId}`, 30_000);
  await waitForComposerIdle(cdp, 60_000);
  await captureScreenshot(cdp, path.join(outputDir, "interaction.png"));

  const activeTurn = await evaluate(
    cdp,
    `window.__talosSmokeInvoke('session.active_turn', { session_id: ${JSON.stringify(sessionId)} }).then((response) => response.active_turn)`,
  );
  assert(
    activeTurn?.status === "completed",
    `Desktop interaction turn did not complete: ${JSON.stringify(activeTurn)}`,
  );
  return {
    sessionId,
    initialEffectiveMode: session.effectiveMode,
    activeTurnStatus: activeTurn.status,
    permissionModes: modeResults,
  };
}

function installSmokeInvoke(cdp) {
  return evaluate(
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
}

async function sendComposerMessage(cdp, message) {
  await setElementValue(cdp, '[data-smoke="chat-composer-input"]', message);
  const sent = await evaluate(
    cdp,
    `(() => {
      const button = document.querySelector('[data-smoke="chat-composer-send"]');
      if (!button || button.disabled) return false;
      button.click();
      return true;
    })()`,
  );
  assert(sent, "Chat composer send button was not actionable.");
}

async function setElementValue(cdp, selector, value) {
  const updated = await evaluate(
    cdp,
    `(() => {
      const element = document.querySelector(${JSON.stringify(selector)});
      if (!element) return false;
      const setter = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value')?.set;
      setter.call(element, ${JSON.stringify(value)});
      element.dispatchEvent(new Event('input', { bubbles: true }));
      element.dispatchEvent(new Event('change', { bubbles: true }));
      return true;
    })()`,
  );
  assert(updated, `Input not found: ${selector}`);
}

async function clickButtonByExactText(cdp, text) {
  const clicked = await evaluate(
    cdp,
    `(() => {
      const text = ${JSON.stringify(text)};
      const button = Array.from(document.querySelectorAll('button')).find(
        (candidate) => (candidate.textContent || '').trim() === text && !candidate.disabled
      );
      if (!button) return false;
      button.click();
      return true;
    })()`,
  );
  assert(clicked, `Button not found: ${text}`);
}

async function clickButtonInUserAction(cdp, text, actionType = "question") {
  const clicked = await evaluate(
    cdp,
    `(() => {
      const section = document.querySelector('section[data-user-action=${JSON.stringify(actionType)}]');
      const text = ${JSON.stringify(text)};
      const button = Array.from(section?.querySelectorAll('button') || []).find(
        (candidate) => (candidate.textContent || '').trim().startsWith(text) && !candidate.disabled
      );
      if (!button) return false;
      button.click();
      return true;
    })()`,
  );
  assert(clicked, `User action button not found: ${text}`);
}

function waitForComposerIdle(cdp, timeoutMs) {
  return waitForExpression(
    cdp,
    `(() => {
      const button = document.querySelector('[data-smoke="chat-composer-send"]');
      return Boolean(button && button.getAttribute('aria-label') === 'Send');
    })()`,
    timeoutMs,
  );
}

async function waitForActiveTurnStatus(cdp, sessionId, status, timeoutMs) {
  const startedAt = Date.now();
  let lastActiveTurn = null;
  let lastError = null;
  while (Date.now() - startedAt < timeoutMs) {
    let response;
    try {
      response = await evaluate(
        cdp,
        `window.__talosSmokeInvoke('session.active_turn', { session_id: ${JSON.stringify(sessionId)} })`,
      );
    } catch (error) {
      lastError = error;
      if (!errorMessage(error).includes("resource not found: session")) {
        throw error;
      }
      await sleep(300);
      continue;
    }
    lastActiveTurn = response.active_turn ?? null;
    if (lastActiveTurn?.status === status) {
      return lastActiveTurn;
    }
    if (lastActiveTurn && ["failed", "cancelled"].includes(lastActiveTurn.status)) {
      throw new Error(`Active turn reached ${lastActiveTurn.status} while waiting for ${status}: ${JSON.stringify(lastActiveTurn)}`);
    }
    await sleep(300);
  }
  throw new Error(
    `Timed out waiting for active turn status ${status}: ${JSON.stringify(lastActiveTurn)}\n${errorMessage(lastError)}`,
  );
}

async function waitForUserActionClosed(cdp, timeoutMs) {
  await waitForExpression(
    cdp,
    "!document.querySelector('section[data-user-action]')",
    timeoutMs,
  );
  await waitForComposerIdle(cdp, timeoutMs);
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
            const timeout = setTimeout(() => {
              if (pending.delete(id)) {
                callReject(new Error(`CDP request timed out: ${method}`));
              }
            }, 30_000);
            pending.set(id, {
              resolve(value) {
                clearTimeout(timeout);
                callResolve(value);
              },
              reject(error) {
                clearTimeout(timeout);
                callReject(error);
              },
            });
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
  const result = await cdp.call("Page.captureScreenshot", { format: "png", captureBeyondViewport: false });
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
