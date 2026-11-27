import { registerDesktopSchemes } from "./file-preview-protocol";
import path from "node:path";
import { hostname } from "node:os";
import { fileURLToPath, pathToFileURL } from "node:url";

import { app, type BaseWindow } from "electron";
import { DesktopAppIcons } from "./desktop-app-icons";
import { createDesktopWindow,desktopWindows } from "./desktop-window";

import { resolveAppServerCommand } from "./app-server-binary";
import { DesktopAppServerController } from "./app-server-controller";
import {
  broadcastActiveTurnChanged,
  broadcastAutomationChanged,
  broadcastMessageQueueChanged,
  broadcastTimelineEvent,
  broadcastRuntimeState,
  broadcastSubscriptionTerminated,
  registerDesktopIpcHandlers,
} from "./desktop-ipc";
import { desktopMediaHost } from "./desktop-media";
import { desktopBrowserHost } from "./browser-host";
import { handleDesktopServerRequest } from "./desktop-server-requests";
import { trustedServerUrl } from "./control-plane";
import { resolveRendererEntryPath } from "./renderer-entry";
import { resolveAppServerEnvironment } from "./shell-environment";

const dirname = path.dirname(fileURLToPath(import.meta.url));
const appIcons = new DesktopAppIcons(dirname);
const rendererDevUrl = process.env.TALOS_DESKTOP_RENDERER_URL;
const serverUrl = process.env.TALOS_DESKTOP_SERVER_URL?.trim();

let controller: DesktopAppServerController | undefined;

function createController(): DesktopAppServerController {
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
    env: { ...env, TALOS_DESKTOP_HOSTNAME: hostname() },
    desktopHost: true,
    clientName: "talos-desktop",
    clientVersion: app.getVersion(),
    clientKind: "desktop",
    onActiveTurnChanged: broadcastActiveTurnChanged,
    onMessageQueueChanged: broadcastMessageQueueChanged,
    onAutomationChanged: broadcastAutomationChanged,
    onEvent: (event) => broadcastTimelineEvent(event),
    onSubscriptionTerminated: (notification) => broadcastSubscriptionTerminated(notification),
    onRuntimeState: (state) => { if(["starting","restarting"].includes(state.kind)) { desktopMediaHost.reset(); void desktopBrowserHost.reset().catch(error=>console.error("Desktop browser reset failed",error)); } broadcastRuntimeState(state); },
    onServerRequest: (request) => handleDesktopServerRequest(request),
  });
}

async function createMainWindow(server?: URL): Promise<BaseWindow> {
  const {window,view} = createDesktopWindow({
    width: 1280,
    height: 820,
    minWidth: 960,
    minHeight: 640,
    title: "Talos",
    icon: appIcons.windowIcon,
  }, {
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: true,
      preload: server ? undefined : path.join(dirname, "preload.cjs"),
  });

  if (server) {
    // The hosted product uses its own HTTP and desktop WebSocket. It has no local IPC bridge.
    await view.webContents.loadURL(server.href);
  } else if (rendererDevUrl) {
    desktopBrowserHost.workspace.setWindow(window,view,rendererDevUrl);
    await view.webContents.loadURL(rendererDevUrl);
  } else {
    const entry=resolveRendererEntryPath({
      appPath: app.getAppPath(),
      dirname,
      isPackaged: app.isPackaged,
    });
    desktopBrowserHost.workspace.setWindow(window,view,pathToFileURL(entry).href);
    await view.webContents.loadFile(entry);
  }

  return window;
}

registerDesktopSchemes();

void app.whenReady().then(async () => {
  const server = serverUrl ? trustedServerUrl(serverUrl) : undefined;
  if (!server) {
    controller = createController();
    registerDesktopIpcHandlers(controller, appIcons);
  }
  // The renderer owns initialization so startup failures remain visible through IPC.
  await createMainWindow(server);

  app.on("activate", () => {
    if (desktopWindows.size === 0) {
      void createMainWindow(server).catch((error: unknown) => console.error("Desktop window creation failed", error));
    }
  });
}).catch((error: unknown) => console.error("Desktop startup failed", error));

let quitState: "running" | "closing" | "closed" = "running";
app.on("before-quit", (event) => {
  if (quitState === "closed") { return; }
  event.preventDefault();
  if (quitState === "closing") { return; }
  quitState = "closing";
  desktopMediaHost.reset();
  void (controller?.close() ?? Promise.resolve())
    .finally(()=>desktopBrowserHost.reset())
    .catch((error: unknown) => console.error("Desktop host shutdown failed", error))
    .finally(() => { quitState = "closed"; app.quit(); });
});

app.on("window-all-closed", () => {
  if (process.platform !== "darwin" || quitState !== "running") {
    app.quit();
  }
});
