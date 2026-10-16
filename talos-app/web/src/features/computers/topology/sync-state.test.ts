import { afterEach, expect, it, vi } from "vitest";
import { AppProtocol } from "@talos/sdk-core";
import { topologyMetrics } from "@/shared/test/computer-topology.fixture";
import { computerSyncState, controlPlaneSyncState } from "./observations";
import { localSyncState, syncObservationState, SyncTopologyState } from "./sync-state";

const now = topologyMetrics.observedAt;
const status: AppProtocol.SyncStatusResponse = {
  configuredEnabled: true, connection: "connected", peerId: "peer-a",
  pendingChanges: 0, lastAcknowledgedAt: null, lastError: null,
  localComputerId: "manager-a", canClaimLocalHistory: false,
};
afterEach(() => vi.useRealTimers());

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
