import { evaluate } from "./desktop-smoke-evaluate.mjs";
export async function waitForPage(port, timeoutMs) {
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

export function connectCdp(url) {
  const socket = new WebSocket(url);
  let nextId = 1;
  const pending = new Map();
  const listeners = new Map();

  socket.addEventListener("message", (event) => {
    const message = JSON.parse(event.data);
    if (!message.id) {
      for (const listener of listeners.get(message.method) ?? []) listener(message.params);
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
        on(method, listener) {
          const current = listeners.get(method) ?? []; current.push(listener); listeners.set(method, current);
        },
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
          for (const request of pending.values()) request.reject(new Error("CDP connection closed."));
          pending.clear();
          socket.close();
        },
      });
    }, { once: true });
    socket.addEventListener("error", () => reject(new Error("Failed to connect to Electron CDP.")), { once: true });
  });
}

export async function waitForExpression(cdp, expression, timeoutMs) {
  const startedAt = Date.now();
  while (Date.now() - startedAt < timeoutMs) {
    if (await evaluate(cdp, expression)) {
      return;
    }
    await sleep(200);
  }
  throw new Error(`Timed out waiting for expression: ${expression}`);
}

export async function waitForEvaluate(cdp, expression, timeoutMs) {
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


function sleep(ms) { return new Promise((resolve) => setTimeout(resolve, ms)); }
function errorMessage(error) { return error instanceof Error ? error.message : String(error); }
