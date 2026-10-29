import { AppProtocol, TalosApiError } from "@talos/sdk-core";
import { ipcMain, shell } from "electron";

import { PreviewBrowserHost } from "./preview-browser";
import { FilePreviewProtocol } from "./file-preview-protocol";
import {desktopWindows,isDesktopRenderer} from "./desktop-window";
import {desktopMediaHost} from "./desktop-media";
import {desktopBrowserHost} from "./browser-host";
import type { DesktopBrowserRequest } from "@talos/sdk-app-server/desktop";
import type { DesktopAppServerController } from "./app-server-controller";
import { assertDesktopRendererInvoke } from "./ipc-guard";
import { TALOS_IPC, type TalosRuntimeState } from "./ipc";

export function registerDesktopIpcHandlers(runtime: DesktopAppServerController): void {
  const browserPreviews = new PreviewBrowserHost(session => filePreviews.installSession(session), TALOS_IPC.previewBrowserState);
  const filePreviews = new FilePreviewProtocol(runtime, () => browserPreviews.clear());
  filePreviews.install();
  ipcMain.handle(TALOS_IPC.previewBrowser, (event, operation: unknown) => ipcResponse(async () => {
    assertProductRenderer(event); return browserPreviews.operate(operation, event.sender);
  }));
  ipcMain.handle(TALOS_IPC.previewTargets, (event, resource: unknown) => ipcResponse(async () => {
    assertProductRenderer(event); return filePreviews.targets(resource, event.sender);
  }));
  ipcMain.handle(TALOS_IPC.previewOpen, (event, request: unknown) => ipcResponse(async () => {
    assertProductRenderer(event); await filePreviews.open(request, event.sender);
  }));
  ipcMain.handle(TALOS_IPC.browserRequest, (event,request:DesktopBrowserRequest) => ipcResponse(async()=>{
    const workspace=desktopBrowserHost.workspace;
    if(event.senderFrame!==event.sender.mainFrame || !workspace.accepts(event.sender))throw new Error("Only the product renderer may present browser targets.");
    if(request.kind!=="bind")return workspace.request(request);
    workspace.unbind();const version=workspace.contextVersion;
    const options=assertDesktopRendererInvoke("user/current",{}, {actorCredentials:request.actorCredentials});
    const actor=await runtime.invoke<AppProtocol.UserCurrentResponse>("user/current",{}, {actorCredentials:options.actorCredentials});
    if(!workspace.accepts(event.sender)||workspace.contextVersion!==version)throw new Error("Browser workspace binding superseded.");
    return workspace.bind(actor.user.user_id);
  }));
  ipcMain.handle(TALOS_IPC.desktopFrameAck,(event,requestId:unknown)=>{if(typeof requestId!=="string"||event.senderFrame!==event.sender.mainFrame)throw new Error("Invalid media acknowledgement.");desktopMediaHost.acknowledge(requestId,event.sender);});
  ipcMain.handle(TALOS_IPC.workerIdentity, () => ipcResponse(() => runtime.invoke("desktop/workerIdentity", null)));
  ipcMain.handle(TALOS_IPC.connect, () => ipcResponse(() => runtime.connect()));
  ipcMain.handle(
    TALOS_IPC.invoke,
    (event, method: unknown, params: unknown, options: unknown) => ipcResponse(async () => {
      assertProductRenderer(event);
      const invokeOptions = assertDesktopRendererInvoke(method, params, options);
      const stream=typeof params==="object" && params!==null && "streamId" in params ? String(params.streamId):null;
      if(method==="desktop/streamOpen"&&stream)desktopMediaHost.own(stream,event.sender,()=>{
        void runtime.invoke("desktop/streamClose",{streamId:stream},{actorCredentials:invokeOptions.actorCredentials})
          .catch(error=>console.error("Desktop media cleanup failed",error));
      });
      if (method === "auth/logout" || method === "auth/session/clear") await filePreviews.clear();
      const previewEpoch = filePreviews.epoch;
      try {
        const result = await runtime.invoke(invokeOptions.method, params, {actorCredentials:invokeOptions.actorCredentials});
        if (method === "file/preview/prepare") filePreviews.remember(result as AppProtocol.FilePreviewDescriptor, invokeOptions.actorCredentials, event.sender, previewEpoch);
        if (method === "file/preview/close") filePreviews.forget((params as AppProtocol.FilePreviewCloseRequest).previewId);
        if (method === "user/current") await filePreviews.accountChanged((result as AppProtocol.UserCurrentResponse).user.user_id, invokeOptions.actorCredentials);
        return result;
      } catch(error) {if(method==="desktop/streamOpen"&&stream)desktopMediaHost.release(stream,event.sender);throw error;}
      finally {if(method==="desktop/streamClose"&&stream)desktopMediaHost.release(stream,event.sender,false);}
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

function assertProductRenderer(event: Electron.IpcMainInvokeEvent): void {
  if (event.senderFrame !== event.sender.mainFrame || !isDesktopRenderer(event.sender)) throw new Error("Only the product renderer may invoke Desktop APIs.");
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
  for (const {view} of desktopWindows) {
    view.webContents.send(TALOS_IPC.event, event);
  }
}

export function broadcastSubscriptionTerminated(notification: AppProtocol.SubscriptionTerminatedNotification): void {
  for (const {view} of desktopWindows) {
    view.webContents.send(TALOS_IPC.subscriptionTerminated, notification);
  }
}

export function broadcastRuntimeState(state: TalosRuntimeState): void {
  for (const {view} of desktopWindows) {
    view.webContents.send(TALOS_IPC.runtimeState, state);
  }
}

export function broadcastActiveTurnChanged(notification: AppProtocol.SessionActiveTurnChangedNotification): void {
  for (const {view} of desktopWindows) view.webContents.send(TALOS_IPC.activeTurnChanged, notification);
}
export function broadcastMessageQueueChanged(notification: AppProtocol.MessageQueueChangedNotification): void {
  for (const {view} of desktopWindows) view.webContents.send(TALOS_IPC.messageQueueChanged, notification);
}
