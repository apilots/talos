import { useEffect } from "react";
import { useInfiniteQuery, useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { createElectronAppServerClient } from "@talos/sdk-app-server/desktop";
import { TalosApiError, AppProtocol } from "@talos/sdk-core";
import { getTalosClient } from "@/shared/api/client";
import { getDesktopBridge } from "@/shared/api/desktop-client";
import { withManagementTimeout } from "@/shared/api/desktop-product-owner";
import { managedWorkerKey } from "@/shared/api/product-queries";
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
  const queryClient = useQueryClient();
  const managedNodeId = queryClient.getQueryData<AppProtocol.ManagedWorkerResponse>(managedWorkerKey(scope))?.instance?.nodeId;
  const list = useInfiniteQuery({
    queryKey: ["talos", scope, "computers", query], initialPageParam: null as string | null,
    queryFn: async ({ pageParam }) => {
      const server = await getTalosClient({ service: "server" });
      return withManagementTimeout((signal) => server.computers.list({ query: query || null, cursor: pageParam, limit: 20 }, signal));
    },
    select: (data) => {
      let removed = 0;
      const pages = data.pages.map((page) => {
        const filtered = page.data.filter(
          (node) => node.managedWorkerPhase == null && (!managedNodeId || node.nodeId !== managedNodeId),
        );
        removed += page.data.length - filtered.length;
        return filtered.length === page.data.length ? page : { ...page, data: filtered };
      });
      if (removed === 0) return data;
      return {
        ...data,
        pages: pages.map((page, index) =>
          index === 0 ? { ...page, total: Math.max(0, page.total - removed) } : page,
        ),
      };
    },
    getNextPageParam: (page) => page.nextCursor ?? undefined, refetchInterval: 5_000, retry: false,
  });
  // Directory observations are memory-only and unusable after a failed read.
  return { ...list, data: list.error ? undefined : list.data };
}
export function isDirectoryMiss(error: unknown) {
  return error instanceof TalosApiError && (
    error.code === AppProtocol.ErrorCode.ResourceNotFound ||
    error.code === AppProtocol.ErrorCode.ResourceExpired ||
    error.code === AppProtocol.ErrorCode.PermissionDenied
  );
}

export function useComputer(nodeId: string | undefined, listedNode?: AppProtocol.NodeInfo) {
  const scope = useProductScope();
  const listed = listedNode?.nodeId === nodeId ? listedNode : undefined;
  const query = useQuery<AppProtocol.NodeInfo>({ queryKey: ["talos", scope, "computer", nodeId],
    placeholderData: listed,
    queryFn: async () => {
      const server = await getTalosClient({ service: "server" });
      return withManagementTimeout((signal) => server.computers.read({ nodeId: nodeId! }, signal));
    }, enabled: Boolean(nodeId), refetchInterval: 5_000, retry: false });
  // Placeholders only bridge the initial read; failures withdraw cached detail.
  return { ...query, data: query.error ? undefined : query.data };
}

/** Local identity, hardware and readiness; never authoritative remote ownership or registration. */
export function useLocalComputer() {
  const scope = useProductScope();
  const bridge = getDesktopBridge();
  const runtime = useAppStore((state) => state.connection.workerRuntimeState);
  const identity = useQuery({ queryKey: ["talos", scope, "local-computer"], enabled: Boolean(bridge),
    queryFn: () => createElectronAppServerClient(bridge!).desktop.workerIdentity(), refetchInterval: 5_000, retry: false });
  return { nodeId: bridge && identity.data ? identity.data.nodeId ?? "embedded" : undefined, runtime, identity };
}

export function useComputers() {
  const { t } = useTranslation("computers");
  const scope = useProductScope();
  const selectedId = useSelectedComputerId();
  const select = useAppStore((state) => state.selectComputer);
  const bridge = getDesktopBridge();
  const { nodeId: desktopId, runtime, identity } = useLocalComputer();
  const sync = useSyncStatus();
  // Desktop identity is local process state, not a replicated Computer directory.
  const localId = bridge ? desktopId ?? "embedded" : sync.data?.localComputerId ?? undefined;
  const list = useComputerList();
  // This local observation is not cached as a directory record and grants no
  // registration/ownership rights. Hardware and readiness always come from Desktop.
  const directoryLocal = !list.error
    ? list.data?.pages.flatMap((page) => page.data).find((node) => node.nodeId === localId)
    : undefined;
  const local: AppProtocol.NodeInfo | undefined = bridge && localId ? {
    nodeId: localId, displayName: t("local"), mode: "embedded", endpoint: null,
    status: "enabled", health: "unknown", leaseExpiresAt: null, lastHeartbeatAt: null,
    createdAt: 0, updatedAt: 0, ownerUserId: null,
    credentialExpiresAt: null,
    ...directoryLocal,
    available: runtime?.kind === "ready",
    systemInfo: identity.data?.systemInfo ?? null,
  } : undefined;
  const directoryNodes = list.data?.pages.flatMap((page) => page.data) ?? [];
  const nodes = local
    ? [local, ...directoryNodes.filter((node) => node.nodeId !== localId)]
    : directoryNodes;
  useEffect(() => {
    if (selectedId === "") return;
    if (selectedId && !(selectedId === "embedded" && localId && localId !== "embedded")) return;
    if (localId) {
      select(scope, localId);
    } else if (list.data?.pages[0]?.total === 1 && nodes.length === 1 && nodes[0].managedWorkerPhase == null) {
      select(scope, nodes[0].nodeId);
    }
  }, [list.data, localId, nodes, scope, select, selectedId]);
  const selected = useComputer(selectedId, directoryNodes.find((node) => node.nodeId === selectedId));
  const clear = useAppStore((state) => state.clearComputer);
  const queryClient = useQueryClient();
  const managedNodeId = queryClient.getQueryData<AppProtocol.ManagedWorkerResponse>(managedWorkerKey(scope))?.instance?.nodeId;
  const selectedIsManaged = Boolean(
    selectedId && ((managedNodeId && selectedId === managedNodeId) || selected.data?.managedWorkerPhase != null),
  );
  useEffect(() => {
    if (!selectedId) return;
    if (selectedIsManaged) {
      if (localId && localId !== selectedId) {
        select(scope, localId);
      } else {
        clear(scope, selectedId);
      }
      return;
    }
    if (!(bridge && identity.isPending) && !list.error && selectedId !== localId && isDirectoryMiss(selected.error)) {
      clear(scope, selectedId);
    }
  }, [bridge, clear, identity.isPending, list.error, localId, scope, select, selected.error, selectedId, selectedIsManaged]);
  const selectedNode = selectedIsManaged
    ? undefined
    : selectedId === localId && local
      ? { ...local, ...(!list.error ? selected.data : undefined), available: local.available, systemInfo: local.systemInfo }
      : list.data ? selected.data : undefined;
  const effectiveSelectedId = selectedIsManaged ? "" : selectedId;
  const quickNodes = [...new Map([selectedNode, ...nodes.filter((node) => node.nodeId !== selectedNode?.nodeId)]
    .filter((node): node is AppProtocol.NodeInfo => node != null && node.managedWorkerPhase == null && (!managedNodeId || node.nodeId !== managedNodeId))
    .map((node) => [node.nodeId, node])).values()]
    .sort((a, b) => Number(b.nodeId === localId) - Number(a.nodeId === localId)).slice(0, 3);
  return { scope, selectedId: effectiveSelectedId, selected: selectedNode, localId, localNode: local, nodes, quickNodes,
    total: list.data?.pages[0]?.total ?? nodes.length, list, runtime, identity,
    error: list.error ?? selected.error ?? identity.error, sync,
    select: (nodeId: string) => select(scope, nodeId),
  };
}
export function useCanManageComputer() {
  const nodeId = useSelectedComputerId();
  const query = useComputer(nodeId);
  const local = useSyncStatus();
  const desktop = useLocalComputer();
  const data = query.data;
  const connection = useAppStore((state) => state.connection);
  // Local configuration is authorized by the Desktop process boundary, not by
  // a synthetic directory row. All other targets still require live Server ownership.
  if (nodeId && nodeId === desktop.nodeId) return true;
  if (getDesktopBridge() && desktop.identity.isPending) return undefined;
  if (isDirectoryMiss(query.error)) return false;
  if (query.isPending) return undefined;
  if (!data) return false;
  if (data.ownerUserId) return data.ownerUserId === connection.userId;
  // Only the backend's current-host identity grants access to an unregistered
  // local Worker. Deployment mode is not an ownership or authorization proof.
  if (data.nodeId !== "embedded") return false;
  return connection.authStatus === "open" || connection.authRole === "admin" || local.data?.localComputerId === "embedded";
}
