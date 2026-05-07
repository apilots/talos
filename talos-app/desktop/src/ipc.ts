import type { AppProtocol } from "@talos/sdk-core";

export const TALOS_IPC = {
  connect: "talos:connect",
  event: "talos:event",
  invoke: "talos:invoke",
  runtimeState: "talos:runtime-state",
  subscriptionTerminated: "talos:subscription-terminated",
} as const;

export type TalosRuntimeStateKind = "starting" | "ready" | "restarting" | "stopped" | "error";

export interface TalosRuntimeState {
  kind: TalosRuntimeStateKind;
  message: string | null;
}

export interface TalosDesktopBridge {
  connect(): Promise<AppProtocol.InitializeResponse>;
  invoke<T>(
    method: AppProtocol.ClientMethod,
    params: unknown,
    options?: { actorCredentials?: AppProtocol.ActorCredentials | null },
  ): Promise<T>;
  onEvent(handler: (envelope: AppProtocol.ApplicationEventEnvelope) => void): () => void;
  onSubscriptionTerminated(handler: (envelope: AppProtocol.SubscriptionTerminatedEnvelope) => void): () => void;
  onRuntimeState(handler: (state: TalosRuntimeState) => void): () => void;
}
