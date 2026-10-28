import { unwrapTalosApiResponse } from "@talos/sdk-core";
import type { IpcRendererEvent } from "electron";
import { contextBridge, ipcRenderer } from "electron";

import type { TalosDesktopBridge, TalosRuntimeState } from "./ipc";
import { TALOS_IPC } from "./ipc";
import type { AppProtocol } from "@talos/sdk-core";

const bridge: TalosDesktopBridge = {
  preview: {
    browser: operation => ipcRenderer.invoke(TALOS_IPC.previewBrowser, operation).then(response => unwrapTalosApiResponse<import("@talos/sdk-app-server/desktop").PreviewBrowserState | null>(response)),
    onBrowserState: handler => {
      const listener = (_event: IpcRendererEvent, state: import("@talos/sdk-app-server/desktop").PreviewBrowserState) => handler(state);
      ipcRenderer.on(TALOS_IPC.previewBrowserState, listener); return () => ipcRenderer.off(TALOS_IPC.previewBrowserState, listener);
    },
    targets: resource => ipcRenderer.invoke(TALOS_IPC.previewTargets, resource).then(response => unwrapTalosApiResponse<import("@talos/sdk-app-server/desktop").PreviewOpenTarget[]>(response)),
    open: request => ipcRenderer.invoke(TALOS_IPC.previewOpen, request).then(response => unwrapTalosApiResponse<void>(response)),
  },
  browserWorkspace: {
    request: request => ipcRenderer.invoke(TALOS_IPC.browserRequest, request),
    onState: handler => {
      const listener = (_event:IpcRendererEvent, state:import("@talos/sdk-app-server/desktop").DesktopBrowserState) => handler(state);
      ipcRenderer.on(TALOS_IPC.browserState, listener);return () => ipcRenderer.off(TALOS_IPC.browserState, listener);
    },
  },
  onDesktopFrame:(handler)=>{
    const listener=(_event:IpcRendererEvent,frame:AppProtocol.DesktopMediaFrame & {requestId:string})=>{
      handler(frame);
      void ipcRenderer.invoke(TALOS_IPC.desktopFrameAck,frame.requestId).catch(()=>{ /* A closed stream may have already withdrawn this acknowledgement. */ });
    };
    ipcRenderer.on(TALOS_IPC.desktopFrame,listener);return ()=>ipcRenderer.off(TALOS_IPC.desktopFrame,listener);
  },
  workerIdentity: () => ipcRenderer.invoke(TALOS_IPC.workerIdentity) as Promise<AppProtocol.ApiResponse<AppProtocol.DesktopWorkerIdentityResponse>>,
  connect: () => ipcRenderer.invoke(TALOS_IPC.connect) as Promise<AppProtocol.ApiResponse<AppProtocol.InitializeResponse>>,
  invoke: <T>(
    method: AppProtocol.ClientMethod,
    params: unknown,
    options?: { actorCredentials?: AppProtocol.ActorCredentials | null },
  ) => ipcRenderer.invoke(TALOS_IPC.invoke, method, params, options) as Promise<AppProtocol.ApiResponse<T>>,
  openExternal: (url: string) => ipcRenderer.invoke(TALOS_IPC.openExternal, url) as Promise<void>,
  onActiveTurnChanged: (handler) => {
    const listener = (_event: IpcRendererEvent, notice: AppProtocol.SessionActiveTurnChangedNotification) => handler(notice);
    ipcRenderer.on(TALOS_IPC.activeTurnChanged, listener); return () => ipcRenderer.off(TALOS_IPC.activeTurnChanged, listener);
  },
  onMessageQueueChanged: (handler) => {
    const listener = (_event: IpcRendererEvent, notice: AppProtocol.MessageQueueChangedNotification) => handler(notice);
    ipcRenderer.on(TALOS_IPC.messageQueueChanged, listener); return () => ipcRenderer.off(TALOS_IPC.messageQueueChanged, listener);
  },
  onEvent: (handler) => {
    const listener = (_event: IpcRendererEvent, timelineEvent: AppProtocol.TimelineEvent) => {
      handler(timelineEvent);
    };
    ipcRenderer.on(TALOS_IPC.event, listener);
    return () => ipcRenderer.off(TALOS_IPC.event, listener);
  },
  onSubscriptionTerminated: (handler) => {
    const listener = (_event: IpcRendererEvent, notification: AppProtocol.SubscriptionTerminatedNotification) => {
      handler(notification);
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
