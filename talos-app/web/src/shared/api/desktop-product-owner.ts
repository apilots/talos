import type { QueryClient } from "@tanstack/react-query";
import { AppProtocol, TalosApiError } from "@talos/sdk-browser";
import type { TalosWebClient } from "./client";
import { managedWorkerKey, sessionComputer, type SessionDetail } from "./product-queries";
import { sessionRouteComputer } from "./session-route";
import { productComputerRecord } from "./computer-target";

export type ProductTarget = {
  nodeId?: string | null;
  sessionId?: string | null;
  /** Only retained project metadata may fall back after an authoritative directory miss. */
  readOnly?: boolean | "retainedProjects";
  service?: "files" | "models";
};

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

/** Product ownership follows the selected Computer, independently of replication. */
export function desktopProductOwner(
  local: TalosWebClient, cloud: TalosWebClient, queries: QueryClient, scope: string, _canUseTalo: boolean, signedIn = _canUseTalo,
  _controlPlaneEndpoint?: string, localComputer: () => Promise<string | null> = () => Promise.resolve("embedded"), selectedComputer?: string,
) {
  const forNode = async (nodeId?: string | null): Promise<TalosWebClient> => {
    const computer = nodeId ?? selectedComputer;
    if (!computer || computer === "embedded") return local;
    const managed = queries.getQueryData<AppProtocol.ManagedWorkerResponse>(managedWorkerKey(scope))?.instance;
    if (computer !== managed?.nodeId && computer === await localComputer()) return local;
    if (!signedIn) throw new TalosApiError({ code: AppProtocol.ErrorCode.Unauthenticated, message: "Sign in to access another Computer.", localization: null });
    return cloud.forComputer?.(computer) ?? cloud;
  };
  const resolve = async (target: ProductTarget): Promise<TalosWebClient> => {
    const computer = (target.sessionId ? sessionRouteComputer(target.sessionId) : undefined)
      ?? target.nodeId ?? (target.sessionId ? sessionComputer(queries, scope, target.sessionId) : undefined);
    return forNode(computer);
  };
  const readSession = async (sessionId: string): Promise<SessionDetail> => {
    const client = await resolve({ sessionId });
    const response = await client.sessions.get({ sessionId });
    return { ...response,
      session: productComputerRecord(response.session, sessionComputer(queries, scope, sessionId) ?? selectedComputer),
      owner: client === local ? "desktop" : "server" };
  };
  return { readSession, resolve };
}
