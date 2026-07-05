import { useQueryClient } from "@tanstack/react-query";

import { chatKeys, useConnectionKey } from "./useChatQueries";
import { getSession } from "../api";
import { hasRealSessionTitle } from "@/shared/types/domain";
import type { SessionInfo } from "@/shared/types/protocol";

const TITLE_REFRESH_DELAYS_MS = [400, 1_000, 2_000, 4_000] as const;

export function shouldRefreshSessionTitleAfterTurn(
  title: string | undefined,
  serverMessageCount: number | undefined,
) {
  return (serverMessageCount ?? 0) === 0 && !hasRealSessionTitle(title);
}

export function useSessionQueryInvalidation(sessionId: string) {
  const connectionKey = useConnectionKey();
  const queryClient = useQueryClient();

  async function invalidateSessionQueries() {
    await queryClient.invalidateQueries({ queryKey: chatKeys.sessions(connectionKey) });
    await queryClient.invalidateQueries({ queryKey: chatKeys.session(connectionKey, sessionId) });
    await queryClient.invalidateQueries({ queryKey: chatKeys.activeTurn(connectionKey, sessionId) });
    await queryClient.invalidateQueries({ queryKey: chatKeys.sessionHistory(connectionKey, sessionId) });
    await queryClient.invalidateQueries({ queryKey: chatKeys.liveRecovery(connectionKey, sessionId) });
  }

  async function refreshSessionTitleAfterTurn({
    serverMessageCount,
    title,
  }: {
    serverMessageCount?: number;
    title?: string;
  }) {
    if (!shouldRefreshSessionTitleAfterTurn(title, serverMessageCount)) {
      return;
    }

    for (const delayMs of TITLE_REFRESH_DELAYS_MS) {
      await delay(delayMs);
      await queryClient.invalidateQueries({ queryKey: chatKeys.session(connectionKey, sessionId) });
      const session = await queryClient.fetchQuery({
        queryKey: chatKeys.session(connectionKey, sessionId),
        queryFn: () => getSession(sessionId),
      });
      updateCachedSessionLists(session);
      await queryClient.invalidateQueries({ queryKey: chatKeys.sessions(connectionKey) });
      await queryClient.refetchQueries({
        queryKey: chatKeys.sessions(connectionKey),
        type: "active",
      });

      if (hasRealSessionTitle(session.title)) {
        return;
      }
    }
  }

  function updateCachedSessionLists(session: SessionInfo) {
    queryClient.setQueriesData<SessionInfo[]>(
      { queryKey: chatKeys.sessions(connectionKey) },
      (sessions) =>
        sessions?.map((entry) =>
          entry.id === session.id ? session : entry,
        ),
    );
  }

  return {
    connectionKey,
    invalidateSessionQueries,
    refreshSessionTitleAfterTurn,
    queryClient,
  };
}

function delay(ms: number) {
  return new Promise((resolve) => window.setTimeout(resolve, ms));
}
