import { randomUUID } from "node:crypto";
import path from "node:path";
import { fileURLToPath } from "node:url";

import type { AppProtocol } from "@talos/sdk-core";
import { app, BrowserWindow, ipcMain } from "electron";

import { DesktopAppServerController } from "./app-server-controller";
import { TALOS_IPC, type TalosRuntimeState } from "./ipc";

const dirname = path.dirname(fileURLToPath(import.meta.url));
const rendererDevUrl = process.env.TALOS_DESKTOP_RENDERER_URL;

let controller: DesktopAppServerController | undefined;

function createController(): DesktopAppServerController {
  return new DesktopAppServerController({
    command: process.env.TALOS_APP_SERVER_BIN,
    launchToken: process.env.TALOS_DESKTOP_LAUNCH_TOKEN ?? randomUUID(),
    clientName: "talos-desktop",
    clientVersion: app.getVersion(),
    onEvent: (envelope) => broadcastEvent(envelope),
    onRuntimeState: (state) => broadcastRuntimeState(state),
  });
}

async function createMainWindow(): Promise<BrowserWindow> {
  const window = new BrowserWindow({
    width: 1280,
    height: 820,
    minWidth: 960,
    minHeight: 640,
    title: "Talos",
    webPreferences: {
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: true,
      preload: path.join(dirname, "preload.js"),
    },
  });

  if (rendererDevUrl) {
    await window.loadURL(rendererDevUrl);
  } else {
    await window.loadFile(path.resolve(dirname, "../../web/dist/index.html"));
  }

  return window;
}

function registerIpcHandlers(runtime: DesktopAppServerController): void {
  ipcMain.handle(TALOS_IPC.connect, () => runtime.connect());
  ipcMain.handle(
    TALOS_IPC.invoke,
    (_event, method: AppProtocol.ClientMethod, params: unknown) => runtime.invoke(method, params),
  );
}

function broadcastEvent(envelope: AppProtocol.ApplicationEventEnvelope): void {
  for (const window of BrowserWindow.getAllWindows()) {
    window.webContents.send(TALOS_IPC.event, envelope);
  }
}

function broadcastRuntimeState(state: TalosRuntimeState): void {
  for (const window of BrowserWindow.getAllWindows()) {
    window.webContents.send(TALOS_IPC.runtimeState, state);
  }
}

app.whenReady().then(async () => {
  controller = createController();
  registerIpcHandlers(controller);
  await createMainWindow();
  await controller.connect();

  app.on("activate", async () => {
    if (BrowserWindow.getAllWindows().length === 0) {
      await createMainWindow();
    }
  });
});

app.on("before-quit", () => {
  void controller?.close();
});

app.on("window-all-closed", () => {
  if (process.platform !== "darwin") {
    app.quit();
  }
});
