import { useEffect } from "react";
import { useInfiniteQuery, useQuery } from "@tanstack/react-query";
import { createElectronAppServerClient } from "@talos/sdk-app-server/desktop";
import type { AppProtocol } from "@talos/sdk-core";
import { getTalosClient } from "@/shared/api/client";
import { getDesktopBridge } from "@/shared/api/desktop-client";
import { useAppStore } from "@/shared/store/app-store";
import { connectionScope } from "@/shared/store/connection-scope";

export function useProductScope() {
  return useAppStore((state) => connectionScope(state.connection));
}
export function useEnvironmentKey() {
  const scope = useProductScope();
  const nodeId = useSelectedComputerId();
  return JSON.stringify([scope, nodeId]);
}
export function useSelectedComputerId() {
  return useAppStore((state) => state.selectedComputers[connectionScope(state.connection)]);
}
export function useSyncStatus() {
  const scope = useProductScope();
  const auth = useAppStore((state) => state.connection.authStatus);
  return useQuery({ queryKey: ["talos", scope, "sync"], queryFn: () => getTalosClient().sync.status(),
    enabled: auth === "authenticated" || auth === "open", refetchInterval: 5_000, retry: false });
}

export function useAccountSyncMetrics() {
  const scope = useProductScope();
  const auth = useAppStore((state) => state.connection.authStatus);
  const enabled = auth === "authenticated";
  const query = useQuery({ queryKey: ["talos", scope, "sync-metrics"],
    queryFn: () => getTalosClient().sync.metrics(), enabled, refetchInterval: 5_000, retry: false });
  // Query data is already keyed by account and management host. Retain identity
  // only: a failed observation must never keep stale peers or counts live.
  return { ...query, data: enabled && !query.error ? query.data : undefined,
    endpoint: enabled ? query.data?.endpoint : undefined, enabled };
}

export function useComputerList(query = "") {
  const scope = useProductScope();
  return useInfiniteQuery({
    queryKey: ["talos", scope, "computers", query], initialPageParam: null as string | null,
    queryFn: ({ pageParam }) => getTalosClient().computers.list({ query: query || null, cursor: pageParam, limit: 20 }),
    getNextPageParam: (page) => page.nextCursor ?? undefined, refetchInterval: 5_000,
  });
}
export function useComputer(nodeId: string | undefined) {
  const scope = useProductScope();
  return useQuery({ queryKey: ["talos", scope, "computer", nodeId],
    queryFn: () => getTalosClient().computers.read({ nodeId: nodeId! }), enabled: Boolean(nodeId), refetchInterval: 5_000 });
}
export function useComputers() {
  const scope = useProductScope();
  const selectedId = useSelectedComputerId();
  const select = useAppStore((state) => state.selectComputer);
  const connection = useAppStore((state) => state.connection);
  const bridge = getDesktopBridge();
  const runtime = connection.workerRuntimeState;
  const identity = useQuery({ queryKey: ["talos", scope, "local-computer"], enabled: Boolean(bridge),
    queryFn: () => createElectronAppServerClient(bridge!).desktop.workerIdentity(), refetchInterval: 5_000 });
  const sync = useSyncStatus();
  // Local is relative to the connected management host, for Desktop and Web alike.
  const localId = sync.data?.localComputerId ?? (bridge && connection.authStatus === "open" ? "embedded" : undefined);
  const list = useComputerList();
  const nodes = list.data?.pages.flatMap((page) => page.data) ?? [];
  useEffect(() => {
    if (selectedId && !(selectedId === "embedded" && localId && localId !== "embedded")) return;
    if (localId) {
      select(scope, localId);
    } else if (list.data?.pages[0]?.total === 1 && nodes.length === 1) {
      select(scope, nodes[0].nodeId);
    }
  }, [bridge, list.data, localId, nodes, scope, select, selectedId]);
  const selected = useComputer(selectedId);
  const local = useComputer(localId);
  const quickNodes = [...new Map([local.data, selected.data, ...nodes]
    .filter((node): node is AppProtocol.NodeInfo => Boolean(node)).map((node) => [node.nodeId, node])).values()].slice(0, 3);
  return { scope, selectedId, selected: selected.data, localId, nodes, quickNodes,
    total: list.data?.pages[0]?.total ?? 0, list, runtime, identity,
    error: selected.error ?? list.error ?? identity.error, sync,
    select: (nodeId: string) => select(scope, nodeId),
  };
}
export function useCanManageComputer() {
  const nodeId = useSelectedComputerId();
  const query = useComputer(nodeId);
  const local = useSyncStatus();
  const data = query.data;
  const connection = useAppStore((state) => state.connection);
  if (query.isPending) return undefined;
  if (!data) return false;
  if (data.ownerUserId) return data.ownerUserId === connection.userId;
  // Only the backend's current-host identity grants access to an unregistered
  // local Worker. Deployment mode is not an ownership or authorization proof.
  if (data.nodeId !== "embedded") return false;
  return connection.authStatus === "open" || connection.authRole === "admin" || local.data?.localComputerId === "embedded";
}
