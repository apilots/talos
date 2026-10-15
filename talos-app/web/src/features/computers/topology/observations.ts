import type { AppProtocol } from "@talos/sdk-core";

export type ComputerService = "controlPlane" | "sync";
export type FlowLayer = "all" | "execution" | "sync";

/** Presentation joins only to the authorized directory; presence grants no permissions. */
export function computerPeers(
  metrics: AppProtocol.SyncMetricsResponse | undefined,
  computerId: string,
) {
  return (
    metrics?.peers.filter((peer) => peer.computerIds.includes(computerId)) ?? []
  );
}

export function controlPlanePeer(
  metrics: AppProtocol.SyncMetricsResponse | undefined,
) {
  return metrics?.peers.find(
    (peer) => peer.peerId === metrics.controlPlanePeerId,
  );
}

export function observationsDelayed(metrics: AppProtocol.SyncMetricsResponse) {
  return (
    !metrics.complete ||
    metrics.lastFlushedAt === null ||
    metrics.observedAt - metrics.lastFlushedAt > 15
  );
}

export function computerPlatform(node: AppProtocol.NodeInfo) {
  const names = { macOs: "macOS", linux: "Linux" };
  return (
    node.systemInfo?.osVersion ??
    (node.systemInfo ? names[node.systemInfo.os] : undefined)
  );
}
