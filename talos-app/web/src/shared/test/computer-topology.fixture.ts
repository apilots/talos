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

const observedAt = Math.floor(Date.now() / 1000);

export const topologyMetrics: AppProtocol.SyncMetricsResponse = {
  endpoint: "https://sync.example",
  controlPlanePeerId: null,
  observedAt,
  lastFlushedAt: observedAt - 1,
  complete: true,
  metadata: { computers: 2, projects: 1, sessions: 3, files: 0 },
  peers: [
    {
      peerId: "peer-a",
      computerIds: ["manager-a"],
      connections: 1,
      documentRooms: 2,
      lastActivityAt: observedAt - 5,
      lastHeartbeatAt: observedAt - 2,
      inboundBytesPerSecond: 20,
      outboundBytesPerSecond: 10,
      updatesPerSecond: 1,
      commitP95Ms: 3,
    },
  ],
};
