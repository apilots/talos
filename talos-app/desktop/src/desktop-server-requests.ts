import type { AppProtocol } from "@talos/sdk-core";
import { BrowserWindow } from "electron";

export async function handleDesktopServerRequest(
  request: AppProtocol.JsonRpcRequest,
  launchToken: string,
): Promise<unknown> {
  switch (request.method) {
    case "client.auth.refresh":
      return { token: launchToken };
    case "client.user_input.request":
      return requestDesktopUserInput(request.params as AppProtocol.RequestUserInputParams);
    default:
      throw new Error(`Unsupported desktop server request: ${request.method}`);
  }
}

function requestDesktopUserInput(
  params: AppProtocol.RequestUserInputParams,
): Promise<AppProtocol.RequestUserInputResponse> {
  return new Promise((resolve, reject) => {
    const parent = activeWindow();
    const modal = new BrowserWindow({
      parent,
      modal: Boolean(parent),
      width: 560,
      height: 360,
      resizable: false,
      minimizable: false,
      maximizable: false,
      title: "Talos input request",
      webPreferences: {
        contextIsolation: true,
        nodeIntegration: false,
        sandbox: true,
      },
    });

    let settled = false;
    const finish = (result: AppProtocol.RequestUserInputResponse | null) => {
      if (settled) {
        return;
      }
      settled = true;
      modal.close();
      if (!result) {
        reject(new Error("Desktop user input request was cancelled."));
        return;
      }
      resolve(result);
    };

    modal.webContents.on("will-navigate", (event, url) => {
      const parsed = new URL(url);
      if (parsed.protocol !== "talos-input:") {
        return;
      }
      event.preventDefault();
      if (parsed.hostname === "submit") {
        finish({ text: parsed.searchParams.get("text") ?? "" });
        return;
      }
      finish(null);
    });
    modal.on("closed", () => finish(null));
    void modal.loadURL(userInputModalUrl(params.prompt));
  });
}

function activeWindow(): BrowserWindow | undefined {
  return BrowserWindow.getFocusedWindow() ?? BrowserWindow.getAllWindows()[0];
}

function userInputModalUrl(prompt: string): string {
  const html = `<!doctype html>
<html>
  <head>
    <meta charset="utf-8" />
    <style>
      body { margin: 0; font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif; background: #f7f4ee; color: #201a16; }
      main { box-sizing: border-box; display: flex; min-height: 100vh; flex-direction: column; gap: 18px; padding: 28px; }
      h1 { margin: 0; font-size: 18px; }
      p { margin: 0; color: #6f6259; line-height: 1.45; }
      textarea { box-sizing: border-box; min-height: 128px; width: 100%; resize: none; border: 1px solid #c9beb4; border-radius: 14px; padding: 12px; font: inherit; background: #fffdf8; color: inherit; }
      footer { display: flex; justify-content: flex-end; gap: 10px; }
      button { border: 0; border-radius: 999px; padding: 10px 18px; font: inherit; cursor: pointer; }
      .secondary { background: #e9e0d8; color: #4d433c; }
      .primary { background: #1f513f; color: white; }
    </style>
  </head>
  <body>
    <main>
      <h1>Talos needs your input</h1>
      <p id="prompt"></p>
      <textarea id="input" autofocus></textarea>
      <footer>
        <button class="secondary" id="cancel" type="button">Cancel</button>
        <button class="primary" id="submit" type="button">Submit</button>
      </footer>
    </main>
    <script>
      const prompt = ${JSON.stringify(prompt)};
      document.getElementById("prompt").textContent = prompt;
      document.getElementById("submit").addEventListener("click", () => {
        const text = document.getElementById("input").value;
        location.href = "talos-input://submit?text=" + encodeURIComponent(text);
      });
      document.getElementById("cancel").addEventListener("click", () => {
        location.href = "talos-input://cancel";
      });
    </script>
  </body>
</html>`;
  return `data:text/html;charset=utf-8,${encodeURIComponent(html)}`;
}
