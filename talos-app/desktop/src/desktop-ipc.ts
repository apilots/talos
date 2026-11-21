import { AppProtocol, TalosApiError } from "@talos/sdk-core";
import { createBrowserTalosClient } from "@talos/sdk-browser";
import { DESKTOP_CLOUD_ORIGIN } from "@talos/sdk-app-server/desktop";
import { controlPlaneFetch } from "./control-plane";
import { ipcMain, protocol, shell } from "electron";

import { isAppIconId } from "../../shared/app-icons";
import { PreviewBrowserHost } from "./preview-browser";
import { FilePreviewProtocol } from "./file-preview-protocol";
import {desktopWindows,isDesktopRenderer} from "./desktop-window";
import {desktopMediaHost} from "./desktop-media";
import {desktopBrowserHost} from "./browser-host";
import type { DesktopBrowserRequest } from "@talos/sdk-app-server/desktop";
import type { DesktopAppServerController } from "./app-server-controller";
import type { DesktopAppIcons } from "./desktop-app-icons";
import { assertDesktopRendererInvoke } from "./ipc-guard";
import { TALOS_IPC, type TalosRuntimeState } from "./ipc";

export function registerDesktopIpcHandlers(
  runtime: DesktopAppServerController,
  appIcons: Pick<DesktopAppIcons, "setAppIcon">,
): void {
  const browserPreviews = new PreviewBrowserHost(session => filePreviews.installSession(session), TALOS_IPC.previewBrowserState);
  const filePreviews = new FilePreviewProtocol(runtime, () => browserPreviews.clear());
  const cloudFetch = controlPlaneFetch(runtime);
  protocol.handle("talos-cloud", async request => {
    const response = await cloudFetch(request);
    // Fetch has already decoded the network response body.
    const headers = new Headers(response.headers);
    headers.delete("content-encoding");
    headers.delete("content-length");
    headers.set("Access-Control-Allow-Origin", "*");
    return new Response(response.body, { status: response.status, headers });
  });
  ipcMain.handle(TALOS_IPC.previewPrepareCloud, (event, request: AppProtocol.FilePreviewPrepareRequest, credentials: unknown) =>
    ipcResponse(async () => {
      assertProductRenderer(event);
      const { actorCredentials } = assertDesktopRendererInvoke("file/preview/prepare", request, { actorCredentials: credentials });
      if (!actorCredentials) throw new Error("A signed-in user is required for a cloud preview.");
      const epoch = filePreviews.epoch;
      const actor = await runtime.invoke<AppProtocol.UserCurrentResponse>("user/current", {}, { actorCredentials });
      const client = createBrowserTalosClient({ baseUrl: DESKTOP_CLOUD_ORIGIN, fetch: cloudFetch,
        getToken: () => actorCredentials.credential.token, getUserId: () => actor.user.user_id });
      const descriptor = await client.files.preview.prepare(request);
      const native = { ...descriptor, previewId: "server-" + descriptor.previewId };
      filePreviews.remember(native, actorCredentials, event.sender, epoch, {
        read: part => client.files.preview.read({ ...part, previewId: descriptor.previewId }),
        close: () => client.files.preview.close({ previewId: descriptor.previewId }),
      });
      return native;
    }));
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
  ipcMain.handle(TALOS_IPC.setAppIcon, (event, id: unknown) => ipcResponse(async () => {
    assertProductRenderer(event);
    if (!isAppIconId(id)) {
      throw new TalosApiError({
        code: AppProtocol.ErrorCode.InvalidRequest,
        message: "Choose a built-in desktop app icon.",
        localization: null,
      });
    }
    appIcons.setAppIcon(id);
    return null;
  }, "The desktop app icon could not be applied; technical details are available in the desktop log."));
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
      if (method === "file/preview/read") {
        const part = params as AppProtocol.FilePreviewReadRequest;
        const cloud = filePreviews.cloudResource(part.previewId, event.sender);
        if (cloud) return cloud.read(part);
      }
      if (method === "file/preview/close") {
        const part = params as AppProtocol.FilePreviewCloseRequest;
        const cloud = filePreviews.cloudResource(part.previewId, event.sender);
        filePreviews.forget(part.previewId);
        if (cloud) return cloud.close(part);
      }
      const previewEpoch = filePreviews.epoch;
      try {
        const result = await runtime.invoke(invokeOptions.method, params, {actorCredentials:invokeOptions.actorCredentials});
        if (method === "file/preview/prepare") filePreviews.remember(result as AppProtocol.FilePreviewDescriptor, invokeOptions.actorCredentials, event.sender, previewEpoch);
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
async function ipcResponse<T>(
  operation: () => Promise<T>,
  failureMessage = "The desktop backend connection failed; technical details are available in the desktop log.",
): Promise<AppProtocol.ApiResponse<T>> {
  try {
    return { status: "success", data: await operation() };
  } catch (error) {
    if (error instanceof TalosApiError) {
      return { status: "error", error: error.apiError };
    }
    console.error("Desktop IPC operation failed", error);
    return { status: "error", error: {
      code: AppProtocol.ErrorCode.InternalError,
      message: failureMessage,
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

export function broadcastAutomationChanged(notification: AppProtocol.AutomationChangedNotification): void {
  for (const {view} of desktopWindows) view.webContents.send(TALOS_IPC.automationChanged, notification);
}
