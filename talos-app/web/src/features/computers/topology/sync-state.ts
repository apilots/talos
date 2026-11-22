import { AppProtocol } from "@talos/sdk-core";

/** Presentation only: heartbeat proves transport liveness, never convergence. */
export enum SyncTopologyState {
  Unknown = "unknown",
  Idle = "idle",
  Syncing = "syncing",
  Disconnected = "disconnected",
  Conflict = "conflict",
  Error = "error",
  Pending = "pending",
  Connecting = "connecting",
  Disabled = "disabled",
}

export const syncStateLabel: Record<SyncTopologyState, string> = {
  [SyncTopologyState.Unknown]: "syncUnknown",
  [SyncTopologyState.Idle]: "syncIdle",
  [SyncTopologyState.Syncing]: "syncActive",
  [SyncTopologyState.Disconnected]: "syncDisconnected",
  [SyncTopologyState.Conflict]: "syncConflict",
  [SyncTopologyState.Error]: "syncFailed",
  [SyncTopologyState.Pending]: "syncWaiting",
  [SyncTopologyState.Connecting]: "syncConnecting",
  [SyncTopologyState.Disabled]: "syncDisabled",
};

export function localSyncState(
  status?: AppProtocol.SyncStatusResponse,
): SyncTopologyState {
  if (!status) return SyncTopologyState.Unknown;
  if (status.connection === "disabled") return SyncTopologyState.Disabled;
  if (
    status.lastError?.code === AppProtocol.ErrorCode.Conflict ||
    status.lastError?.code === AppProtocol.ErrorCode.IdempotencyConflict
  ) {
    return SyncTopologyState.Conflict;
  }
  switch (status.connection) {
    case "connecting":
      return SyncTopologyState.Connecting;
    case "disconnected":
      return SyncTopologyState.Disconnected;
    case "connected":
      if (status.lastError) return SyncTopologyState.Error;
      return status.pendingChanges > 0
        ? SyncTopologyState.Pending
        : SyncTopologyState.Idle;
  }
}

export function observationsFresh(
  metrics: AppProtocol.SyncMetricsResponse | undefined,
  now = Math.floor(Date.now() / 1000),
): metrics is AppProtocol.SyncMetricsResponse {
  return metrics !== undefined && Math.abs(now - metrics.observedAt) <= 15;
}

/** Local errors apply only when the caller identified this host's peer/Computer. */
export function syncObservationState(
  metrics: AppProtocol.SyncMetricsResponse | undefined,
  peers: AppProtocol.SyncPeerMetrics[],
  localStatus?: AppProtocol.SyncStatusResponse,
  now = Math.floor(Date.now() / 1000),
): SyncTopologyState {
  const local = localSyncState(localStatus);
  if (localStatus && local !== SyncTopologyState.Idle) return local;
  if (!observationsFresh(metrics, now)) return SyncTopologyState.Unknown;
  const live = peers.filter((peer) => peer.connections > 0);
  if (!live.length) return SyncTopologyState.Unknown;
  return live.some(
    (peer) =>
      peer.lastActivityAt !== null &&
      metrics.observedAt - peer.lastActivityAt >= 0 &&
      metrics.observedAt - peer.lastActivityAt < 60,
  )
    ? SyncTopologyState.Syncing
    : SyncTopologyState.Idle;
}

/** Aggregate only current live observations; a missing report never means caught up. */
export function syncProgressSummary(
  metrics: AppProtocol.SyncMetricsResponse | undefined,
  peers: AppProtocol.SyncPeerMetrics[],
  localStatus?: AppProtocol.SyncStatusResponse,
  now = Math.floor(Date.now() / 1000),
) {
  const live = observationsFresh(metrics, now)
    ? [...new Map(peers.filter((peer) => peer.connections > 0).map((peer) => [peer.peerId, peer])).values()]
    : [];
  const reports = live.flatMap((peer) => peer.progress ? [peer.progress] : []);
  const state = reports.some((report) => report.state === "recovering")
    ? "recovering"
    : reports.some((report) => report.state === "behind")
      ? "behind"
      : live.length > 0 && reports.length === live.length
        ? "caughtUp"
        : "unknown";
  const lags = reports.flatMap((report) =>
    report.state === "behind" && report.downloadLagMs !== null ? [report.downloadLagMs] : [],
  );
  const remotePending = live.filter((peer) => peer.peerId !== localStatus?.peerId);
  const pendingKnown = remotePending.every((peer) => peer.progress);
  const pendingUploads = pendingKnown && (live.length > 0 || localStatus)
    ? remotePending.reduce((total, peer) => total + (peer.progress?.pendingUploads ?? 0), localStatus?.pendingChanges ?? 0)
    : null;
  return {
    state,
    downloadLagMs: lags.length ? Math.max(...lags) : null,
    pendingUploads,
    observedAt: reports.length ? Math.min(...reports.map((report) => report.observedAt)) : null,
  };
}
