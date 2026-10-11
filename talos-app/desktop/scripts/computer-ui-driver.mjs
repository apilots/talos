/** Real isolated Electron / Chrome surfaces, controlled only through CDP and public APIs. */
import { spawn } from "node:child_process";
import { createWriteStream, writeFileSync } from "node:fs";
import { createInterface } from "node:readline";
import path from "node:path";
import { connectCdp, waitForPage, waitForExpression } from "./desktop-cdp.mjs";
import { evaluate } from "./desktop-smoke-evaluate.mjs";

const surfaces = new Map();
async function command(request) {
  if (request.method === "launch") {
    const { name, port, home, config, resourceDir, report, spki, webUrl, executable, mainPort } = request.params;
    const web = Boolean(webUrl);
    const desktop = path.resolve(import.meta.dirname, "..");
    const args = [`--remote-debugging-port=${port}`, `--ignore-certificate-errors-spki-list=${spki}`];
    if (!web && mainPort) args.push(`--inspect=${mainPort}`);
    if (web) args.push(`--user-data-dir=${home}`, "--no-first-run", "--no-default-browser-check", "--window-size=1280,820", webUrl);
    else {
      args.push(`--user-data-dir=${path.join(home, "electron-profile")}`);
      if (!executable) args.unshift(desktop);
    }
    const env = { ...process.env, TALOS_HOME: home, TALOS_CONFIG: config, TALOS_RESOURCE_DIR: resourceDir };
    if (!web && !executable) env.TALOS_APP_SERVER_BIN = path.join(resourceDir, "talos-rs/target/debug/app-server");
    else { delete env.TALOS_APP_SERVER_BIN; if (!web) delete env.TALOS_RESOURCE_DIR; }
    const child = spawn(executable || (web ? process.env.TALOS_E2E_CHROME_BIN : path.join(desktop, "node_modules/.bin/electron")), args, { env, cwd: desktop, stdio: ["ignore", "pipe", "pipe"] });
    const log = createWriteStream(path.join(report, `${name}.log`)); child.stdout.pipe(log); child.stderr.pipe(log);
    const surface = { child, log, web, port, cdp: null, main: null }; surfaces.set(name, surface);
    const page = await waitForPage(port, 90_000); surface.cdp = await connectCdp(page.webSocketDebuggerUrl);
    await surface.cdp.call("Runtime.enable"); await surface.cdp.call("Page.enable");
    if (!web) {
      await waitForExpression(surface.cdp, "Boolean(window.talos)", 30_000);
      await evaluate(surface.cdp, "window.talos.connect().then(r=>{if(r.status!==\"success\")throw r.error;return true})");
    }
    if (!web && mainPort) {
      const targets = await fetch(`http://127.0.0.1:${mainPort}/json/list`).then(r => r.json());
      surface.main = await connectCdp(targets[0].webSocketDebuggerUrl);
      // Isolate the OS browser opener only; the real UI, native auth RPC, polling and cache remain unchanged.
      await evaluate(surface.main, '(()=>{const {shell}=process.getBuiltinModule("module").createRequire(process.cwd()+"/package.json")("electron");shell.openExternal=async(url)=>{globalThis.__e2eAuthUrl=url;};return true;})()');
    }
    return { pid: child.pid };
  }
  const { cdp, web } = surfaces.get(request.surface);
  switch (request.method) {
    case "evaluate": return evaluate(cdp, request.params.expression);
    case "wait": await waitForExpression(cdp, request.params.expression, request.params.timeout ?? 60_000); return true;
    case "viewport": await cdp.call("Emulation.setDeviceMetricsOverride", {width:request.params.width,height:request.params.height,deviceScaleFactor:1,mobile:false}); return true;
    case "key": await cdp.call("Page.bringToFront"); await cdp.call("Input.dispatchKeyEvent",{type:"keyDown",key:request.params.key,code:request.params.key,windowsVirtualKeyCode:27,nativeVirtualKeyCode:27}); await cdp.call("Input.dispatchKeyEvent",{type:"keyUp",key:request.params.key,code:request.params.key,windowsVirtualKeyCode:27,nativeVirtualKeyCode:27}); return true;
    case "navigate": await cdp.call("Page.navigate", { url: request.params.url }); return true;
    case "reload": await cdp.call("Page.reload"); return true;
    case "screenshot": { const result = await cdp.call("Page.captureScreenshot", { format: "png" }); writeFileSync(request.params.path, Buffer.from(result.data, "base64")); return true; }
    case "login": {
      const { ingress, cloud, token } = request.params;
      await waitForExpression(cdp, 'Array.from(document.querySelectorAll("button")).some(b=>b.textContent.includes("Continue with Talos Cloud")&&!b.disabled)', 60_000);
      const previousTargets = web ? new Set((await fetch(`http://127.0.0.1:${surfaces.get(request.surface).port}/json/list`).then(r => r.json())).map(target => target.id)) : null;
      if (!web) await evaluate(surfaces.get(request.surface).main, 'globalThis.__e2eAuthUrl=null');
      const button = await evaluate(cdp, '(()=>{const b=Array.from(document.querySelectorAll("button")).find(b=>b.textContent.includes("Continue with Talos Cloud")&&!b.disabled);const r=b.getBoundingClientRect();return {x:r.x+r.width/2,y:r.y+r.height/2};})()');
      await cdp.call("Page.bringToFront");
      await cdp.call("Input.dispatchMouseEvent", {type:"mousePressed",button:"left",clickCount:1,...button});
      await cdp.call("Input.dispatchMouseEvent", {type:"mouseReleased",button:"left",clickCount:1,...button});
      let loginUrl;
      let approvalTarget;
      if (web) {
        const port = surfaces.get(request.surface).port;
        const deadline = Date.now() + 30_000;
        while (Date.now() < deadline) {
          const targets = await fetch(`http://127.0.0.1:${port}/json/list`).then(r => r.json());
          approvalTarget = targets.find(target => !previousTargets.has(target.id) && target.url.startsWith(cloud) && target.url.includes("session_id="));
          loginUrl = approvalTarget?.url;
          if (loginUrl) break;
          await new Promise(resolve => setTimeout(resolve, 200));
        }
      } else {
        const main = surfaces.get(request.surface).main;
        await waitForExpression(main, 'Boolean(globalThis.__e2eAuthUrl)', 30_000);
        loginUrl = await evaluate(main, 'globalThis.__e2eAuthUrl');
      }
      if (!loginUrl) throw new Error("Login UI did not open device approval");
      const params = new URL(loginUrl).searchParams;
      const approval = await fetch(cloud + "/v1/auth/device/approve", { method: "POST", headers: { "Content-Type": "application/json", Authorization: `Bearer ${token}` }, body: JSON.stringify({ session_id: params.get("session_id"), user_code: params.get("user_code") }) });
      if ((await approval.json()).status !== "approved") throw new Error("Device approval failed");
      if (approvalTarget) await cdp.call("Target.closeTarget", {targetId:approvalTarget.id});
      await cdp.call("Page.bringToFront");
      // AuthPage must poll and save identity itself; do not inject a token or reload around it.
      await waitForExpression(cdp, 'Boolean(document.querySelector("[data-computer-id]"))', 60_000);
      return web ? true : evaluate(cdp, 'window.talos.workerIdentity().then(r=>r.data)');
    }
    case "draft": {
      await waitForExpression(cdp, 'Boolean(document.querySelector("textarea:not(:disabled)"))', 30_000);
      await evaluate(cdp, `(() => {const textarea=document.querySelector("textarea");if(!textarea)throw new Error("Composer missing");Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype,"value").set.call(textarea,${JSON.stringify(request.params.text)});textarea.dispatchEvent(new Event("input",{bubbles:true}));})()`); return true;
    }
    case "send": {
      await waitForExpression(cdp, 'Boolean(document.querySelector("[data-smoke=chat-composer-send]:not(:disabled)"))', 30_000);
      await evaluate(cdp, 'document.querySelector("[data-smoke=chat-composer-send]").click()');
      await waitForExpression(cdp, web ? 'location.pathname.startsWith("/sessions/")' : 'location.hash.startsWith("#/sessions/")', 30_000);
      return evaluate(cdp, web ? 'location.pathname.split("/").at(-1)' : 'location.hash.split("/").at(-1)');
    }
    case "select": {
      await evaluate(cdp, `(() => {const button=document.querySelector('aside [data-computer-id="${request.params.nodeId}"]');if(button)button.click();else throw new Error("Quick Computer missing");})()`);
      await waitForExpression(cdp, `${web ? 'location.pathname==="/"' : 'location.hash==="#/"'} && Boolean(document.querySelector("main h1")) && Boolean(document.querySelector('[data-testid=computer-context] [data-computer-id="${request.params.nodeId}"]'))`, 30_000); return true;
    }
    default: throw new Error("Unknown UI operation");
  }
}
const lines = createInterface({ input: process.stdin });
try {
  for await (const line of lines) {
    const request = JSON.parse(line);
    try { process.stdout.write(JSON.stringify({ id: request.id, result: (await command(request)) ?? null }) + "\n"); }
    catch (error) {
      const cdp = surfaces.get(request.surface)?.cdp;
      const view = cdp ? await evaluate(cdp, '({path:location.pathname,hash:location.hash,text:document.body?.innerText})').catch(() => null) : null;
      process.stdout.write(JSON.stringify({ id: request.id, error: { code: error.code, message: error.message, view } }) + "\n");
    }
  }
} finally {
  for (const surface of surfaces.values()) {
    surface.cdp?.close();
    surface.main?.close();
    if (surface.child.exitCode === null) {
      const exited = new Promise((resolve) => surface.child.once("exit", resolve));
      surface.child.kill("SIGINT");
      const timeout = setTimeout(() => surface.child.kill("SIGKILL"), 10_000);
      await exited; clearTimeout(timeout);
    }
    surface.log.end();
  }
}
