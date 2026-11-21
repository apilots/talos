import type { QueryClient } from "@tanstack/react-query";
import { AppProtocol, TalosApiError } from "@talos/sdk-browser";
import type { TalosWebClient } from "./client";
import { managedWorkerKey, sessionDetailKey, type SessionDetail } from "./product-queries";

export type ProductTarget = { nodeId?: string | null; sessionId?: string | null; readOnly?: boolean };

function isMissing(error: unknown) {
  return error instanceof TalosApiError && error.code === AppProtocol.ErrorCode.ResourceNotFound;
}

function isUnavailable(error: unknown) {
  return error instanceof TypeError || (error instanceof TalosApiError &&
    (error.code === AppProtocol.ErrorCode.ServiceUnavailable || error.code === AppProtocol.ErrorCode.Timeout));
}

/** Resolve an immutable target using Server identity and the authorized local projection. */
export function desktopProductOwner(
  local: TalosWebClient, cloud: TalosWebClient, queries: QueryClient, scope: string, canUseTalo: boolean, signedIn = canUseTalo,
) {
  const managedKey = managedWorkerKey(scope);
  const knownManaged = () => queries.getQueryData<AppProtocol.ManagedWorkerResponse>(managedKey)?.instance;
  const readManaged = () => queries.fetchQuery({
    queryKey: managedKey, queryFn: () => cloud.computers.readManagedWorker(), retry: false, staleTime: 5_000,
  });
  const forNode = async (nodeId?: string | null, allowOfflineRead = false) => {
    if (!nodeId || nodeId === "embedded") return local;
    if (knownManaged()?.nodeId === nodeId) return cloud;
    const computer = queries.getQueryData<AppProtocol.NodeInfo>(["talos", scope, "computer", nodeId]);
    if (computer?.managedWorkerPhase != null) return cloud;
    if (!canUseTalo) return local;
    const sync = queries.getQueryData<AppProtocol.SyncStatusResponse>(["talos", scope, "sync"]);
    if (sync?.localComputerId === nodeId) return local;
    try {
      const directory = await queries.fetchQuery({
        queryKey: ["talos", scope, "server-computer", nodeId], queryFn: () => cloud.computers.read({ nodeId }), retry: false, staleTime: 5_000,
      });
      return directory.managedWorkerPhase != null ? cloud : local;
    } catch (error) {
      // A synced directory omits managedWorkerPhase even for Talo. Only reads can
      // use a local projection until Server confirms the target's execution owner.
      if (allowOfflineRead && isUnavailable(error)) return local;
      throw error;
    }
  };
  const readCloudSession = async (sessionId: string): Promise<SessionDetail> => ({
    ...(await cloud.sessions.get({ sessionId })), owner: "server",
  });
  const readProjectedSession = async (sessionId: string, projection: SessionDetail, allowOfflineRead: boolean): Promise<SessionDetail> => {
    try {
      if ((await forNode(projection.session.nodeId, allowOfflineRead)) === local) return { ...projection, owner: "desktop" };
    } catch (error) {
      // Removing a Talo deletes its directory entry, while Server retains its conversations.
      if (!signedIn || !isMissing(error)) throw error;
      try { return await readCloudSession(sessionId); }
      catch (historyError) {
        if (allowOfflineRead && (isMissing(historyError) || isUnavailable(historyError))) return { ...projection, owner: "desktop" };
        throw historyError;
      }
    }
    return readCloudSession(sessionId);
  };
  const readSession = async (sessionId: string): Promise<SessionDetail> => {
    const cached = queries.getQueryData<SessionDetail>(sessionDetailKey(scope, sessionId));
    if (cached?.owner === "server" || knownManaged()?.conversationSessionId === sessionId) return readCloudSession(sessionId);
    let projection: SessionDetail;
    try {
      projection = await local.sessions.get({ sessionId });
    } catch (error) {
      // Server retains authorized history after its original managed Computer is deleted.
      if (signedIn && isMissing(error)) return readCloudSession(sessionId);
      throw error;
    }
    return readProjectedSession(sessionId, projection, true);
  };
  const resolve = async ({ sessionId, nodeId, readOnly }: ProductTarget) => {
    if (!sessionId) return forNode(nodeId, readOnly);
    const key = sessionDetailKey(scope, sessionId);
    const cached = queries.getQueryData<SessionDetail>(key);
    if (cached?.owner === "server" || knownManaged()?.conversationSessionId === sessionId) return cloud;
    if (cached) {
      const detail = await readProjectedSession(sessionId, cached, Boolean(readOnly));
      if (detail.owner === "server") queries.setQueryData(key, detail);
      return detail.owner === "server" ? cloud : local;
    }
    if (nodeId) return forNode(nodeId, readOnly);
    if (!signedIn) return local;
    try {
      const detail = await queries.ensureQueryData({ queryKey: key, queryFn: () => readSession(sessionId) });
      if (detail.owner === "server") return cloud;
      return readOnly ? local : forNode(detail.session.nodeId);
    } catch (error) {
      if (isMissing(error) && canUseTalo && (await readManaged()).instance?.conversationSessionId === sessionId) return cloud;
      throw error;
    }
  };
  return { readSession, resolve };
}
