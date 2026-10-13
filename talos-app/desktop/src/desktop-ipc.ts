import { AppProtocol, TalosApiError } from "@talos/sdk-core";
import { BrowserWindow, ipcMain, shell } from "electron";

import type { DesktopAppServerController } from "./app-server-controller";
import { assertDesktopRendererInvoke } from "./ipc-guard";
import { TALOS_IPC, type TalosRuntimeState } from "./ipc";

export function registerDesktopIpcHandlers(runtime: DesktopAppServerController): void {
  ipcMain.handle(TALOS_IPC.workerIdentity, () => ipcResponse(() => runtime.invoke("desktop/workerIdentity", null)));
  ipcMain.handle(TALOS_IPC.connect, () => ipcResponse(() => runtime.connect()));
  ipcMain.handle(
    TALOS_IPC.invoke,
    (_event, method: unknown, params: unknown, options: unknown) => ipcResponse(() => {
      const invokeOptions = assertDesktopRendererInvoke(method, params, options);
      return runtime.invoke(invokeOptions.method, params, {
        actorCredentials: invokeOptions.actorCredentials,
      });
    }),
  );
  ipcMain.handle(TALOS_IPC.openExternal, (_event, url: unknown) => {
    if (typeof url !== "string") {
      throw new Error("External URL must be a string.");
    }
    const parsed = new URL(url);
    if (parsed.protocol !== "http:" && parsed.protocol !== "https:") {
      throw new Error(`External URL protocol is not allowed: ${parsed.protocol}`);
    }
    return shell.openExternal(parsed.toString());
  });
}

/** Preserve the public envelope across IPC; native Error properties do not survive it. */
async function ipcResponse<T>(operation: () => Promise<T>): Promise<AppProtocol.ApiResponse<T>> {
  try {
    return { status: "success", data: await operation() };
  } catch (error) {
    if (error instanceof TalosApiError) {
      return { status: "error", error: error.apiError };
    }
    console.error("Desktop IPC operation failed", error);
    return { status: "error", error: {
      code: AppProtocol.ErrorCode.InternalError,
      message: "The desktop backend connection failed; technical details are available in the desktop log.",
      localization: null,
    } };
  }
}

export function broadcastTimelineEvent(event: AppProtocol.TimelineEvent): void {
  for (const window of BrowserWindow.getAllWindows()) {
    window.webContents.send(TALOS_IPC.event, event);
  }
}

export function broadcastSubscriptionTerminated(notification: AppProtocol.SubscriptionTerminatedNotification): void {
  for (const window of BrowserWindow.getAllWindows()) {
    window.webContents.send(TALOS_IPC.subscriptionTerminated, notification);
  }
}

export function broadcastRuntimeState(state: TalosRuntimeState): void {
  for (const window of BrowserWindow.getAllWindows()) {
    window.webContents.send(TALOS_IPC.runtimeState, state);
  }
}

export function broadcastActiveTurnChanged(notification: AppProtocol.SessionActiveTurnChangedNotification): void {
  for (const window of BrowserWindow.getAllWindows()) window.webContents.send(TALOS_IPC.activeTurnChanged, notification);
}
export function broadcastMessageQueueChanged(notification: AppProtocol.MessageQueueChangedNotification): void {
  for (const window of BrowserWindow.getAllWindows()) window.webContents.send(TALOS_IPC.messageQueueChanged, notification);
}
