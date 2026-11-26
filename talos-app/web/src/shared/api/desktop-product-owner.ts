import type { QueryClient } from "@tanstack/react-query";
import { AppProtocol, TalosApiError } from "@talos/sdk-browser";
import { createPersistStorage } from "@/shared/store/persist-storage";
import { isServiceUnavailable } from "./is-service-unavailable";
import type { TalosWebClient } from "./client";
import { managedWorkerKey, sessionDetailKey, type SessionDetail } from "./product-queries";

export type ProductTarget = {
  nodeId?: string | null;
  sessionId?: string | null;
  /** Only retained project metadata may fall back after an authoritative directory miss. */
  readOnly?: boolean | "retainedProjects";
  service?: "files" | "models";
};
type OwnerOptions = Pick<ProductTarget, "readOnly" | "service">;

export const MANAGEMENT_LOOKUP_TIMEOUT_MS = 5_000;

export function withManagementTimeout<T>(
  run: (signal: AbortSignal) => Promise<T>,
  timeoutMs = MANAGEMENT_LOOKUP_TIMEOUT_MS,
): Promise<T> {
  const controller = new AbortController();
  let timedOut = false;
  let timer: ReturnType<typeof setTimeout> | undefined;
  const timeoutError = () =>
    new TalosApiError({
      code: AppProtocol.ErrorCode.Timeout,
      message: "The management Server did not respond in time.",
      localization: null,
    });
  const timeout = new Promise<never>((_, reject) => {
    timer = setTimeout(() => {
      timedOut = true;
      const error = timeoutError();
      controller.abort(error);
      reject(error);
    }, timeoutMs);
  });
  const guarded = Promise.resolve()
    .then(() => run(controller.signal))
    .catch((error: unknown) => {
      if (timedOut) throw timeoutError();
      throw error;
    });
  return Promise.race([guarded, timeout]).finally(() => {
    if (timer !== undefined) clearTimeout(timer);
  });
}

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

function isVerifiedDesktopWorker(queries: QueryClient, scope: string, nodeId: string, endpoint?: string) {
  const sync = queries.getQueryData<AppProtocol.SyncStatusResponse>(["talos", scope, "sync"]);
  if (sync?.localComputerId === nodeId) return true;
  const key = ordinaryWorkerKey(scope, endpoint, nodeId);
  return Boolean(key && withWorkerStorage(storage => storage.getItem(key)) === "1");
}

/** Resolve an immutable target using its session kind or, before admission, its Computer. */
export function desktopProductOwner(
  local: TalosWebClient, cloud: TalosWebClient, queries: QueryClient, scope: string, canUseTalo: boolean, signedIn = canUseTalo,
  controlPlaneEndpoint?: string,
) {
  const managedKey = managedWorkerKey(scope);
  const knownManaged = () => queries.getQueryData<AppProtocol.ManagedWorkerResponse>(managedKey)?.instance;
  const readManaged = () => queries.fetchQuery({
    queryKey: managedKey,
    queryFn: () => withManagementTimeout((signal) => cloud.computers.readManagedWorker(signal)),
    retry: false,
    staleTime: 5_000,
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
          const node = await withManagementTimeout((signal) => cloud.computers.read({ nodeId }, signal));
          const ordinaryKey = ordinaryWorkerKey(scope, controlPlaneEndpoint, nodeId);
          if (ordinaryKey && node.nodeId === nodeId && node.managedWorkerPhase == null) {
            withWorkerStorage(storage => storage.setItem(ordinaryKey, "1"));
          }
          return node;
        },
      });
      return directory.managedWorkerPhase != null ? cloud : local;
    } catch (error) {
      if (!service && readOnly && isServiceUnavailable(error)) return local;
      // A directory miss remains authoritative for Computer details.
      if (!service && readOnly === "retainedProjects" && isMissing(error)) return local;
      throw error;
    }
  };
  // A standard chat can live on either transport. Talo always belongs to the Server.
  const sessionOwner = (kind: AppProtocol.SessionInfo["sessionKind"], source: TalosWebClient) =>
    ({ standard: source, talo: cloud })[kind];

  async function sessionRoute(sessionId: string, admitDraft: boolean): Promise<{
    client: TalosWebClient;
    detail?: AppProtocol.SessionGetResponse;
  }> {
    if (knownManaged()?.conversationSessionId === sessionId) return { client: cloud };
    const cached = queries.getQueryData<SessionDetail>(sessionDetailKey(scope, sessionId));
    if (cached) return { client: sessionOwner(cached.session.sessionKind, cached.owner === "server" ? cloud : local) };

    const readRoute = async (source: TalosWebClient) => {
      const detail = await source.sessions.get({ sessionId });
      const client = sessionOwner(detail.session.sessionKind, source);
      return { client, detail: client === source ? detail : undefined };
    };
    try {
      return await readRoute(local);
    } catch (error) {
      if (signedIn && error instanceof TalosApiError && error.code === AppProtocol.ErrorCode.PermissionDenied) {
        // Local access checks can hide a synchronized Talo after an account downgrade.
        const detail = await cloud.sessions.get({ sessionId });
        if (detail.session.sessionKind !== "talo") throw error;
        return { client: cloud, detail };
      }
      if (!isMissing(error)) throw error;
      if (!signedIn) {
        if (admitDraft) return { client: local };
        throw error;
      }
    }

    // The Server may already hold a session that has not synchronized to Desktop.
    try {
      return await readRoute(cloud);
    } catch (error) {
      if (admitDraft && isMissing(error)) {
        if (knownManaged()?.conversationSessionId === sessionId) return { client: cloud };
        if (canUseTalo && (await readManaged()).instance?.conversationSessionId === sessionId) return { client: cloud };
      }
      throw error;
    }
  }

  const readSession = async (sessionId: string): Promise<SessionDetail> => {
    const { client, detail } = await sessionRoute(sessionId, false);
    const response = detail ?? await client.sessions.get({ sessionId });
    return { ...response, owner: client === cloud ? "server" : "desktop" };
  };
  const resolve = async (target: ProductTarget) => target.sessionId
    ? (await sessionRoute(target.sessionId, true)).client
    : forNode(target.nodeId, target);
  return { readSession, resolve };
}
