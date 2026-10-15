import type { AppProtocol } from "@talos/sdk-core";

export const topologyNodes: AppProtocol.NodeInfo[] = [
  ["manager-a", "Studio", true],
  ["worker-b", "Build Worker", false],
].map(([id, name, available]) => ({
  nodeId: String(id),
  displayName: String(name),
  available: Boolean(available),
  mode: "remote",
  endpoint: null,
  status: "enabled",
  health: "healthy",
  leaseExpiresAt: null,
  lastHeartbeatAt: null,
  createdAt: 1,
  updatedAt: 1,
  ownerUserId: "alice",
  credentialExpiresAt: null,
  systemInfo: null,
}));

export const topologyMetrics: AppProtocol.SyncMetricsResponse = {
  endpoint: "https://sync.example",
  controlPlanePeerId: null,
  observedAt: 1_700_000_000,
  lastFlushedAt: 1_699_999_999,
  complete: true,
  metadata: { computers: 2, projects: 1, sessions: 3, files: 0 },
  peers: [
    {
      peerId: "peer-a",
      computerIds: ["manager-a"],
      connections: 1,
      documentRooms: 2,
      lastActivityAt: 1_699_999_995,
      inboundBytesPerSecond: 20,
      outboundBytesPerSecond: 10,
      updatesPerSecond: 1,
      commitP95Ms: 3,
    },
  ],
};
