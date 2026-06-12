import type { IpcRendererEvent } from "electron";
import { contextBridge, ipcRenderer } from "electron";

import type { TalosDesktopBridge, TalosRuntimeState } from "./ipc";
import { TALOS_IPC } from "./ipc";
import type { AppProtocol } from "@talos/sdk-core";

const bridge: TalosDesktopBridge = {
  connect: () => ipcRenderer.invoke(TALOS_IPC.connect) as Promise<AppProtocol.InitializeResponse>,
  invoke: <T>(
    method: AppProtocol.ClientMethod,
    params: unknown,
    options?: { actorCredentials?: AppProtocol.ActorCredentials | null },
  ) => ipcRenderer.invoke(TALOS_IPC.invoke, method, params, options) as Promise<T>,
  openExternal: (url: string) => ipcRenderer.invoke(TALOS_IPC.openExternal, url) as Promise<void>,
  onEvent: (handler) => {
    const listener = (_event: IpcRendererEvent, timelineEvent: AppProtocol.TimelineEventEnvelope) => {
      handler(timelineEvent);
    };
    ipcRenderer.on(TALOS_IPC.event, listener);
    return () => ipcRenderer.off(TALOS_IPC.event, listener);
  },
  onSubscriptionTerminated: (handler) => {
    const listener = (_event: IpcRendererEvent, envelope: AppProtocol.SubscriptionTerminatedEnvelope) => {
      handler(envelope);
    };
    ipcRenderer.on(TALOS_IPC.subscriptionTerminated, listener);
    return () => ipcRenderer.off(TALOS_IPC.subscriptionTerminated, listener);
  },
  onRuntimeState: (handler) => {
    const listener = (_event: IpcRendererEvent, state: TalosRuntimeState) => {
      handler(state);
    };
    ipcRenderer.on(TALOS_IPC.runtimeState, listener);
    return () => ipcRenderer.off(TALOS_IPC.runtimeState, listener);
  },
};

contextBridge.exposeInMainWorld("talos", bridge);
