import { afterEach, expect, it, vi } from "vitest";
import { AppProtocol } from "@talos/sdk-core";
import { topologyMetrics } from "@/shared/test/computer-topology.fixture";
import { computerSyncState, controlPlaneSyncState } from "./observations";
import { localSyncState, syncObservationState, syncProgressSummary, SyncTopologyState } from "./sync-state";

const now = topologyMetrics.observedAt;
const status: AppProtocol.SyncStatusResponse = {
  configuredEnabled: true, connection: "connected", peerId: "peer-a",
  pendingChanges: 0, lastAcknowledgedAt: null, lastError: null,
  localComputerId: "manager-a", canClaimLocalHistory: false,
};
afterEach(() => vi.useRealTimers());

it("shows explicitly disabled Control Plane Sync without requiring observations", () => {
  const disabled = { ...status, configuredEnabled: false, connection: "disabled" as const };
  expect(controlPlaneSyncState(undefined, disabled)).toBe(SyncTopologyState.Disabled);
  expect(controlPlaneSyncState({ ...topologyMetrics, controlPlanePeerId: null }, disabled)).toBe(SyncTopologyState.Disabled);
  expect(controlPlaneSyncState(undefined)).toBe(SyncTopologyState.Unknown);
  expect(computerSyncState(undefined, "worker-b", disabled)).toBe(SyncTopologyState.Unknown);
});

it("keeps idle heartbeats separate from document flow and expires old snapshots", () => {
  const peer: AppProtocol.SyncPeerMetrics = { ...topologyMetrics.peers[0], lastActivityAt: null, lastHeartbeatAt: now };
  const metrics = { ...topologyMetrics, peers: [peer] };
  expect(syncObservationState(metrics, [peer], undefined, now)).toBe(SyncTopologyState.Idle);
  peer.lastActivityAt = now - 5;
  expect(syncObservationState(metrics, [peer], undefined, now)).toBe(SyncTopologyState.Syncing);
  peer.lastActivityAt = now - 60;
  expect(syncObservationState(metrics, [peer], undefined, now)).toBe(SyncTopologyState.Idle);
  expect(syncObservationState(metrics, [peer], undefined, now + 16)).toBe(SyncTopologyState.Unknown);
  expect(syncObservationState(undefined, [peer], undefined, now)).toBe(SyncTopologyState.Unknown);
  expect(syncObservationState(metrics, [], undefined, now)).toBe(SyncTopologyState.Unknown);
  peer.lastHeartbeatAt = null;
  expect(syncObservationState(metrics, [peer], undefined, now)).toBe(SyncTopologyState.Idle);
});

it("does not treat delayed performance counters or an unavailable Worker as a Sync disconnect", () => {
  const metrics = { ...topologyMetrics, complete: false, lastFlushedAt: null };
  expect(syncObservationState(metrics, metrics.peers, undefined, now)).toBe(SyncTopologyState.Syncing);
  expect(computerSyncState(metrics, "worker-b")).toBe(SyncTopologyState.Unknown);
});

it("scopes pending and typed conflicts to this host, even while it reconnects", () => {
  vi.useFakeTimers(); vi.setSystemTime(now * 1000);
  const conflict = { ...status, connection: "connecting" as const,
    lastError: { code: AppProtocol.ErrorCode.Conflict, message: "Conflict", localization: null } };
  expect(computerSyncState(topologyMetrics, "manager-a", conflict)).toBe(SyncTopologyState.Conflict);
  expect(computerSyncState(topologyMetrics, "worker-b", conflict)).toBe(SyncTopologyState.Unknown);
  expect(localSyncState({ ...conflict, lastError: { ...conflict.lastError, code: AppProtocol.ErrorCode.IdempotencyConflict } })).toBe(SyncTopologyState.Conflict);
  expect(localSyncState({ ...status, pendingChanges: 2 })).toBe(SyncTopologyState.Pending);
  expect(localSyncState({ ...status, lastError: { ...conflict.lastError, code: AppProtocol.ErrorCode.UploadTooLarge } })).toBe(SyncTopologyState.Error);
  expect(localSyncState({ ...conflict, connection: "disabled" })).toBe(SyncTopologyState.Disabled);
  expect(computerSyncState(undefined, "manager-a", { ...status, connection: "disconnected" })).toBe(SyncTopologyState.Disconnected);
});

it("uses the same state for the exact Control Plane peer, including disconnect and recovery", () => {
  vi.useFakeTimers(); vi.setSystemTime(now * 1000);
  const metrics = { ...topologyMetrics, controlPlanePeerId: "peer-a" };
  expect(controlPlaneSyncState(metrics, status)).toBe(SyncTopologyState.Syncing);
  expect(controlPlaneSyncState({ ...metrics, peers: [] }, { ...status, connection: "disconnected" })).toBe(SyncTopologyState.Disconnected);
  expect(controlPlaneSyncState({ ...metrics, peers: [] }, { ...status, connection: "connecting" })).toBe(SyncTopologyState.Connecting);
  expect(controlPlaneSyncState(metrics, status)).toBe(SyncTopologyState.Syncing);
  expect(controlPlaneSyncState({ ...metrics, controlPlanePeerId: "another-peer" }, status)).toBe(SyncTopologyState.Unknown);
});


it("keeps idle download backlog and pending uploads separate from document activity", () => {
  const peer: AppProtocol.SyncPeerMetrics = { ...topologyMetrics.peers[0], lastActivityAt: null,
    progress: { observedAt: now - 20, lastAppliedAt: now - 60, state: "behind", downloadLagMs: 1200, pendingUploads: 4, sourceDelays: [] } };
  const metrics = { ...topologyMetrics, peers: [peer] };
  expect(syncObservationState(metrics, [peer], undefined, now)).toBe(SyncTopologyState.Idle);
  expect(syncProgressSummary(metrics, [peer], undefined, now)).toEqual({ state: "behind", downloadLagMs: 1200, pendingUploads: 4, observedAt: now - 20 });
  peer.progress!.state = "recovering";
  expect(syncObservationState(metrics, [peer], undefined, now)).toBe(SyncTopologyState.Idle);
  expect(syncProgressSummary(metrics, [peer], undefined, now).state).toBe("recovering");
  expect(syncProgressSummary(metrics, [peer], undefined, now + 16)).toEqual({ state: "unknown", downloadLagMs: null, pendingUploads: null, observedAt: null });
  expect(syncProgressSummary(metrics, [peer], { ...status, pendingChanges: 7 }, now + 16).pendingUploads).toBe(7);
});

it("aggregates multiple peers conservatively without double counting local pending work", () => {
  const first: AppProtocol.SyncPeerMetrics = { ...topologyMetrics.peers[0],
    progress: { observedAt: now - 20, lastAppliedAt: now - 60, state: "caughtUp", downloadLagMs: 0, pendingUploads: 3, sourceDelays: [] } };
  const second: AppProtocol.SyncPeerMetrics = { ...first, peerId: "peer-b",
    progress: { ...first.progress!, state: "behind", downloadLagMs: 1500, pendingUploads: 4, observedAt: now - 5 } };
  const third: AppProtocol.SyncPeerMetrics = { ...first, peerId: "peer-c",
    progress: { ...first.progress!, state: "recovering", downloadLagMs: null, pendingUploads: 2 } };
  const metrics = { ...topologyMetrics, peers: [first, second, third] };
  expect(syncProgressSummary(metrics, [first, first, second], { ...status, pendingChanges: 8 }, now))
    .toEqual({ state: "behind", downloadLagMs: 1500, pendingUploads: 12, observedAt: now - 20 });
  expect(syncProgressSummary(metrics, metrics.peers, undefined, now))
    .toEqual({ state: "recovering", downloadLagMs: 1500, pendingUploads: 9, observedAt: now - 20 });
  second.progress = null;
  expect(syncProgressSummary(metrics, [first, second], undefined, now))
    .toEqual({ state: "unknown", downloadLagMs: null, pendingUploads: null, observedAt: now - 20 });
  second.progress = { ...first.progress!, state: "behind", downloadLagMs: null };
  expect(syncProgressSummary(metrics, [second], undefined, now).state).toBe("behind");
  expect(syncProgressSummary(metrics, [{ ...first, connections: 0 }], undefined, now).state).toBe("unknown");
});


it.each(["disconnected", "unobserved"] as const)(
  "retains the exact local outbox beside another live peer when the local peer is %s",
  (localObservation) => {
    const local: AppProtocol.SyncPeerMetrics = { ...topologyMetrics.peers[0], connections: 0 };
    const remote: AppProtocol.SyncPeerMetrics = { ...topologyMetrics.peers[0], peerId: "peer-b",
      progress: { observedAt: now, lastAppliedAt: now, state: "caughtUp", downloadLagMs: 0, pendingUploads: 0, sourceDelays: [] } };
    const peers = localObservation === "disconnected" ? [local, remote] : [remote];
    const metrics = { ...topologyMetrics, peers };
    const localStatus = { ...status, pendingChanges: 7 };
    expect(syncProgressSummary(metrics, peers, localStatus, now).pendingUploads).toBe(7);
    remote.progress = null;
    expect(syncProgressSummary(metrics, peers, localStatus, now).pendingUploads).toBeNull();
  },
);

it("does not reuse a Hub catch-up sample after this exact local peer disconnects", () => {
  const progress: AppProtocol.SyncPeerProgress = { state: "caughtUp", downloadLagMs: 0, pendingUploads: 1,
    observedAt: now, lastAppliedAt: now, sourceDelays: [] };
  const peer = { ...topologyMetrics.peers[0], progress };
  const metrics = { ...topologyMetrics, peers: [peer] };
  for (const connection of ["disconnected", "connecting", "disabled"] as const) {
    const local = { ...status, connection, pendingChanges: 7 };
    expect(syncProgressSummary(metrics, [peer], local, now)).toMatchObject({ state: "unknown", pendingUploads: 7 });
    const remote = { ...peer, peerId: "other-peer" };
    expect(syncProgressSummary(metrics, [remote], local, now)).toMatchObject({ state: "caughtUp", pendingUploads: 8 });
  }
  expect(syncProgressSummary(metrics, [peer], { ...status,
    lastError: { code: AppProtocol.ErrorCode.Conflict, message: "Conflict", localization: null } }, now).state).toBe("caughtUp");
});
