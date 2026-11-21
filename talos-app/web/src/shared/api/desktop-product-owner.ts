import type { QueryClient } from "@tanstack/react-query";
import { AppProtocol, TalosApiError } from "@talos/sdk-browser";
import { createPersistStorage } from "@/shared/store/persist-storage";
import { isServiceUnavailable } from "./is-service-unavailable";
import type { TalosWebClient } from "./client";
import { managedWorkerKey, sessionDetailKey, type SessionDetail } from "./product-queries";

export type ProductTarget = { nodeId?: string | null; sessionId?: string | null; readOnly?: boolean; service?: "files" | "models" };
type OwnerOptions = Pick<ProductTarget, "readOnly" | "service">;

function isMissing(error: unknown) {
  return error instanceof TalosApiError && error.code === AppProtocol.ErrorCode.ResourceNotFound;
}

function withWorkerStorage<T>(access: (storage: Storage) => T) {
  try { return access(createPersistStorage()); }
  catch (error) {
    console.warn("The verified Worker route could not be cached or restored.", error);
  }
}

function ordinaryWorkerKey(scope: string, endpoint: string | undefined, nodeId: string) {
  return endpoint && JSON.stringify(["talos", scope, endpoint, "ordinary-worker", nodeId]);
}

export function isVerifiedDesktopWorker(queries: QueryClient, scope: string, nodeId?: string | null, endpoint?: string) {
  if (!nodeId) return false;
  const sync = queries.getQueryData<AppProtocol.SyncStatusResponse>(["talos", scope, "sync"]);
  if (nodeId === "embedded" || sync?.localComputerId === nodeId) return true;
  const key = ordinaryWorkerKey(scope, endpoint, nodeId);
  return Boolean(key && withWorkerStorage(storage => storage.getItem(key)) === "1");
}

/** Resolve an immutable target using Server identity and the authorized local projection. */
export function desktopProductOwner(
  local: TalosWebClient, cloud: TalosWebClient, queries: QueryClient, scope: string, canUseTalo: boolean, signedIn = canUseTalo,
  controlPlaneEndpoint?: string,
) {
  const managedKey = managedWorkerKey(scope);
  const knownManaged = () => queries.getQueryData<AppProtocol.ManagedWorkerResponse>(managedKey)?.instance;
  const readManaged = () => queries.fetchQuery({
    queryKey: managedKey, queryFn: () => cloud.computers.readManagedWorker(), retry: false, staleTime: 5_000,
  });
  const forNode = async (nodeId?: string | null, { readOnly, service }: OwnerOptions = {}) => {
    if (!nodeId || nodeId === "embedded") return local;
    if (knownManaged()?.nodeId === nodeId) return cloud;
    const computer = queries.getQueryData<AppProtocol.NodeInfo>(["talos", scope, "computer", nodeId]);
    if (computer?.managedWorkerPhase != null) return cloud;
    if (!signedIn || (!canUseTalo && !service)) return local;
    // Ordinary UUIDs never become managed Workers. Files and model capabilities
    // can use that verified route; their services authorize each operation.
    if (isVerifiedDesktopWorker(queries, scope, nodeId, service ? controlPlaneEndpoint : undefined)) return local;
    try {
      const directory = await queries.fetchQuery({
        queryKey: ["talos", scope, "server-computer", controlPlaneEndpoint, nodeId], retry: false, staleTime: 5_000,
        queryFn: async () => {
          const node = await cloud.computers.read({ nodeId });
          const ordinaryKey = ordinaryWorkerKey(scope, controlPlaneEndpoint, nodeId);
          if (ordinaryKey && node.nodeId === nodeId && node.managedWorkerPhase == null) {
            withWorkerStorage(storage => storage.setItem(ordinaryKey, "1"));
          }
          return node;
        },
      });
      return directory.managedWorkerPhase != null ? cloud : local;
    } catch (error) {
      // Synced metadata can display history offline, even for an unclassified Computer.
      if (!service && readOnly && isServiceUnavailable(error)) return local;
      throw error;
    }
  };
  const readCloudSession = async (sessionId: string): Promise<SessionDetail> => ({
    ...(await cloud.sessions.get({ sessionId })), owner: "server",
  });
  const readProjectedSession = async (sessionId: string, projection: SessionDetail, options: OwnerOptions): Promise<SessionDetail> => {
    try {
      if ((await forNode(projection.session.nodeId, options)) === local) return { ...projection, owner: "desktop" };
    } catch (error) {
      // Removing a Talo deletes its directory entry, while Server retains its conversations.
      if (!signedIn || !isMissing(error)) throw error;
      try { return await readCloudSession(sessionId); }
      catch (historyError) {
        if (!options.service && options.readOnly && (isMissing(historyError) || isServiceUnavailable(historyError))) return { ...projection, owner: "desktop" };
        throw historyError;
      }
    }
    return readCloudSession(sessionId);
  };
  const readSession = async (sessionId: string, draftNodeId?: string, service?: ProductTarget["service"]): Promise<SessionDetail> => {
    const cached = queries.getQueryData<SessionDetail>(sessionDetailKey(scope, sessionId));
    if (cached?.owner === "server" || knownManaged()?.conversationSessionId === sessionId) return readCloudSession(sessionId);
    // An uncertain first send can only have created the session on its explicit target.
    if (draftNodeId) {
      if ((await forNode(draftNodeId)) === cloud) return readCloudSession(sessionId);
      return { ...(await local.sessions.get({ sessionId })), owner: "desktop" };
    }
    let projection: SessionDetail;
    try {
      projection = await local.sessions.get({ sessionId });
    } catch (error) {
      // Server retains authorized history after its original managed Computer is deleted.
      if (signedIn && isMissing(error)) return readCloudSession(sessionId);
      throw error;
    }
    return readProjectedSession(sessionId, projection, { readOnly: true, service });
  };
  const resolve = async (target: ProductTarget) => {
    const { sessionId, nodeId, readOnly, service } = target;
    if (!sessionId) return forNode(nodeId, target);
    const key = sessionDetailKey(scope, sessionId);
    const cached = queries.getQueryData<SessionDetail>(key);
    if (cached?.owner === "server" || knownManaged()?.conversationSessionId === sessionId) return cloud;
    if (cached) {
      const detail = await readProjectedSession(sessionId, cached, target);
      if (detail.owner === "server") queries.setQueryData(key, detail);
      return detail.owner === "server" ? cloud : local;
    }
    if (nodeId) return forNode(nodeId, target);
    if (!signedIn) return local;
    try {
      const detail = await queries.ensureQueryData({ queryKey: key, queryFn: () => readSession(sessionId, undefined, service) });
      if (detail.owner === "server") return cloud;
      return readOnly && !service ? local : forNode(detail.session.nodeId, target);
    } catch (error) {
      if (isMissing(error) && canUseTalo && (await readManaged()).instance?.conversationSessionId === sessionId) return cloud;
      throw error;
    }
  };
  return { readSession, resolve };
}
