import type { SessionInfo } from "@talos/sdk-browser";
import { QueryClient } from "@tanstack/react-query";
import { sessionRouteComputer } from "./session-route";

/** Transport ownership reads share the same actor-scoped cache as product observers. */
export const productQueries = new QueryClient({
  defaultOptions: { queries: { retry: 1, refetchOnWindowFocus: false } },
});

export const managedWorkerKey = (scope: string) => ["talos", scope, "managed-worker"] as const;
export const sessionDetailKey = (scope: string, id: string, computerId = sessionRouteComputer(id)) =>
  computerId ? ["talos", scope, "session", id, computerId] as const : ["talos", scope, "session", id] as const;

/** Reuse display metadata only when the existing cache identifies one product. */
function cachedSessionComputer(queries: QueryClient, scope: string, sessionId: string): string | undefined {
  const computers = new Set(queries.getQueriesData<SessionDetail>({
    queryKey: ["talos", scope, "session", sessionId],
  }).flatMap(([, detail]) => detail?.session.nodeId ? [detail.session.nodeId] : []));
  return computers.size === 1 ? [...computers][0] : undefined;
}

export function sessionComputer(queries: QueryClient, scope: string, sessionId: string): string | undefined {
  const managed = queries.getQueryData<import("@talos/sdk-browser").AppProtocol.ManagedWorkerResponse>(managedWorkerKey(scope))?.instance;
  return sessionRouteComputer(sessionId)
    ?? (managed?.conversationSessionId === sessionId ? managed.nodeId : undefined)
    ?? cachedSessionComputer(queries, scope, sessionId);
}

/** Display metadata remains in the original actor and product-scoped query cache. */
export interface SessionDetail {
  session: SessionInfo;
  owner?: "desktop" | "server";
}
