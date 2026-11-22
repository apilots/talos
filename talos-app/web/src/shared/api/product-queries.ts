import type { SessionInfo } from "@talos/sdk-browser";
import { QueryClient } from "@tanstack/react-query";

/** Transport ownership reads share the same actor-scoped cache as product observers. */
export const productQueries = new QueryClient({
  defaultOptions: { queries: { retry: 1, refetchOnWindowFocus: false } },
});

export const managedWorkerKey = (scope: string) => ["talos", scope, "managed-worker"] as const;
export const sessionDetailKey = (scope: string, id: string) => ["talos", scope, "session", id] as const;

/** A standard session can exist only on the Server when Desktop Sync is off. */
export interface SessionDetail {
  session: SessionInfo;
  owner?: "desktop" | "server";
}
