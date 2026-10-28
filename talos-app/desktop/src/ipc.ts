import type { AppProtocol } from "@talos/sdk-core";

export const TALOS_IPC = {
  browserRequest: "talos:browser-workspace-request",
  browserState: "talos:browser-workspace-state",
  desktopFrame:"talos:desktop-frame",
  desktopFrameAck:"talos:desktop-frame-ack",
  connect: "talos:connect",
  workerIdentity: "desktop/workerIdentity",
  event: "talos:event",
  activeTurnChanged: "talos:active-turn-changed",
  messageQueueChanged: "talos:message-queue-changed",
  invoke: "talos:invoke",
  openExternal: "talos:open-external",
  previewTargets: "talos:preview-targets",
  previewOpen: "talos:preview-open",
  previewBrowser: "talos:preview-browser",
  previewBrowserState: "talos:preview-browser-state",
  runtimeState: "talos:runtime-state",
  subscriptionTerminated: "talos:subscription-terminated",
} as const;

export type { TalosRuntimeStateKind, TalosRuntimeState } from "@talos/sdk-app-server/desktop";
import type { TalosRuntimeState } from "@talos/sdk-app-server/desktop";

export interface TalosDesktopBridge {
  preview: import("@talos/sdk-app-server/desktop").DesktopPreviewBridge;
  browserWorkspace: import("@talos/sdk-app-server/desktop").DesktopBrowserBridge;
  onDesktopFrame(handler:(frame:AppProtocol.DesktopMediaFrame)=>void):()=>void;
  workerIdentity(): Promise<AppProtocol.ApiResponse<AppProtocol.DesktopWorkerIdentityResponse>>;
  connect(): Promise<AppProtocol.ApiResponse<AppProtocol.InitializeResponse>>;
  invoke<T>(
    method: AppProtocol.ClientMethod,
    params: unknown,
    options?: { actorCredentials?: AppProtocol.ActorCredentials | null },
  ): Promise<AppProtocol.ApiResponse<T>>;
  openExternal(url: string): Promise<void>;
  onActiveTurnChanged(handler: (notification: AppProtocol.SessionActiveTurnChangedNotification) => void): () => void;
  onMessageQueueChanged(handler: (notification: AppProtocol.MessageQueueChangedNotification) => void): () => void;
  onEvent(handler: (event: AppProtocol.TimelineEvent) => void): () => void;
  onSubscriptionTerminated(handler: (notification: AppProtocol.SubscriptionTerminatedNotification) => void): () => void;
  onRuntimeState(handler: (state: TalosRuntimeState) => void): () => void;
}
