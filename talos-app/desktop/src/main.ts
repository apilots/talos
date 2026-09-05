import path from "node:path";
import { fileURLToPath } from "node:url";

import { app, BrowserWindow } from "electron";

import { resolveAppServerCommand } from "./app-server-binary";
import { DesktopAppServerController } from "./app-server-controller";
import {
  broadcastTimelineEvent,
  broadcastRuntimeState,
  broadcastSubscriptionTerminated,
  registerDesktopIpcHandlers,
} from "./desktop-ipc";
import { handleDesktopServerRequest } from "./desktop-server-requests";
import { resolveRendererEntryPath } from "./renderer-entry";
import { resolveAppServerEnvironment } from "./shell-environment";

const dirname = path.dirname(fileURLToPath(import.meta.url));
const rendererDevUrl = process.env.TALOS_DESKTOP_RENDERER_URL;

let controller: DesktopAppServerController | undefined;

async function createController(): Promise<DesktopAppServerController> {
  const env = resolveAppServerEnvironment({
    baseEnv: process.env,
    packagedResourcesPath: app.isPackaged ? process.resourcesPath : null,
  });
  return new DesktopAppServerController({
    command: resolveAppServerCommand({
      env,
      isPackaged: app.isPackaged,
      platform: process.platform,
      resourcesPath: process.resourcesPath,
    }),
    env,
    clientName: "talos-desktop",
    clientVersion: app.getVersion(),
    clientKind: "desktop",
    onEvent: (event) => broadcastTimelineEvent(event),
    onSubscriptionTerminated: (notification) => broadcastSubscriptionTerminated(notification),
    onRuntimeState: (state) => broadcastRuntimeState(state),
    onServerRequest: (request) => handleDesktopServerRequest(request),
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
      preload: path.join(dirname, "preload.cjs"),
    },
  });

  if (rendererDevUrl) {
    await window.loadURL(rendererDevUrl);
  } else {
    await window.loadFile(resolveRendererEntryPath({
      appPath: app.getAppPath(),
      dirname,
      isPackaged: app.isPackaged,
    }));
  }

  return window;
}

app.whenReady().then(async () => {
  controller = await createController();
  registerDesktopIpcHandlers(controller);
  await controller.connect();
  await createMainWindow();

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
