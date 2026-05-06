import type { AppProtocol } from "@talos/sdk-core";
import { BrowserWindow, ipcMain } from "electron";

import type { DesktopAppServerController } from "./app-server-controller";
import { assertDesktopRendererInvoke } from "./ipc-guard";
import { TALOS_IPC, type TalosRuntimeState } from "./ipc";

export function registerDesktopIpcHandlers(runtime: DesktopAppServerController): void {
  ipcMain.handle(TALOS_IPC.connect, () => runtime.connect());
  ipcMain.handle(
    TALOS_IPC.invoke,
    (_event, method: unknown, params: unknown) => {
      assertDesktopRendererInvoke(method, params);
      return runtime.invoke(method, params);
    },
  );
}

export function broadcastApplicationEvent(envelope: AppProtocol.ApplicationEventEnvelope): void {
  for (const window of BrowserWindow.getAllWindows()) {
    window.webContents.send(TALOS_IPC.event, envelope);
  }
}

export function broadcastSubscriptionTerminated(envelope: AppProtocol.SubscriptionTerminatedEnvelope): void {
  for (const window of BrowserWindow.getAllWindows()) {
    window.webContents.send(TALOS_IPC.subscriptionTerminated, envelope);
  }
}

export function broadcastRuntimeState(state: TalosRuntimeState): void {
  for (const window of BrowserWindow.getAllWindows()) {
    window.webContents.send(TALOS_IPC.runtimeState, state);
  }
}
