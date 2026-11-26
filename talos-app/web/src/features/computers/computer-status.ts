import type { TalosRuntimeState } from "@talos/sdk-app-server/desktop";
import type { AppProtocol } from "@talos/sdk-core";

/** Local process readiness is independent of Server registration and access policy. */
export function computerStatus(
  node: AppProtocol.NodeInfo,
  local = false,
  runtime?: TalosRuntimeState,
) {
  if (local) {
    return {
      label: `localRuntime.${runtime?.kind ?? "unknown"}` as const,
      ready: runtime?.kind === "ready",
      remoteDisabled: node.status === "disabled",
    };
  }
  return {
    label: node.managedWorkerPhase && node.managedWorkerPhase !== "ready"
      ? `talo.phase.${node.managedWorkerPhase}` as const
      : node.status === "disabled" ? "remoteDisabled" as const
        : node.available ? "connected" as const : "offline" as const,
    ready: node.available,
    remoteDisabled: false,
  };
}
