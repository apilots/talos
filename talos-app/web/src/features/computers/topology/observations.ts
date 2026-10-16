import type { AppProtocol } from "@talos/sdk-core";
import { syncObservationState } from "./sync-state";

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

export function computerSyncState(
  metrics: AppProtocol.SyncMetricsResponse | undefined,
  computerId: string,
  status?: AppProtocol.SyncStatusResponse,
) {
  const peers = computerPeers(metrics, computerId);
  const local =
    status?.localComputerId === computerId ||
    peers.some((peer) => peer.peerId === status?.peerId);
  return syncObservationState(metrics, peers, local ? status : undefined);
}

export function controlPlaneSyncState(
  metrics: AppProtocol.SyncMetricsResponse | undefined,
  status?: AppProtocol.SyncStatusResponse,
) {
  const peer = controlPlanePeer(metrics);
  return syncObservationState(
    metrics,
    peer ? [peer] : [],
    metrics?.controlPlanePeerId && metrics.controlPlanePeerId === status?.peerId
      ? status
      : undefined,
  );
}
