import { useRef } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import type { SessionListEntry } from "@talos/sdk-browser";

import { sessionDetailKey, type SessionDetail } from "@/shared/api/product-queries";
import { updateSessionPinned } from "../api";
import { chatKeys, useConnectionKey } from "./useChatQueries";

export function useToggleSessionPinMutation() {
  const connectionKey = useConnectionKey();
  const queryClient = useQueryClient();
  const requestSeqRef = useRef(0);
  const latestRequestBySessionRef = useRef<Map<string, number>>(new Map());
  const mutationKey = ["session-pin", connectionKey] as const;
  const sessionsPredicate = (query: { queryKey: readonly unknown[] }) =>
    query.queryKey.includes("sessions");
  const hasOtherInFlightForSession = (sessionId: string) =>
    queryClient.isMutating({
      mutationKey,
      predicate: (mutation) =>
        (mutation.state.variables as { sessionId?: string } | undefined)?.sessionId === sessionId,
    }) > 1;

  return useMutation({
    mutationKey,
    mutationFn: ({ sessionId, pinned }: { sessionId: string; pinned: boolean }) =>
      updateSessionPinned(sessionId, pinned),
    onMutate: async ({ sessionId, pinned }) => {
      const requestToken = ++requestSeqRef.current;
      latestRequestBySessionRef.current.set(sessionId, requestToken);
      const sessionKey = sessionDetailKey(connectionKey, sessionId);
      await queryClient.cancelQueries({ queryKey: ["talos"], predicate: sessionsPredicate });
      await queryClient.cancelQueries({ queryKey: sessionKey });
      const matchingLists = queryClient.getQueriesData<SessionListEntry[]>({
        queryKey: ["talos"],
        predicate: sessionsPredicate,
      });
      const previousListPinned = matchingLists.map(
        ([queryKey, list]) =>
          [queryKey, list?.find((session) => session.id === sessionId)?.pinned] as const,
      );
      const previousDetailPinned =
        queryClient.getQueryData<SessionDetail>(sessionKey)?.session.pinned;
      queryClient.setQueriesData<SessionListEntry[]>(
        { queryKey: ["talos"], predicate: sessionsPredicate },
        (current) =>
          current?.map((session) => (session.id === sessionId ? { ...session, pinned } : session)),
      );
      queryClient.setQueryData<SessionDetail>(sessionKey, (current) =>
        current ? { ...current, session: { ...current.session, pinned } } : current,
      );
      return { previousListPinned, previousDetailPinned, sessionKey, requestToken };
    },
    onError: (_error, { sessionId, pinned }, context) => {
      if (context?.requestToken !== latestRequestBySessionRef.current.get(sessionId)) {
        return;
      }
      for (const [queryKey, prevPinned] of context?.previousListPinned ?? []) {
        if (prevPinned === undefined) {
          continue;
        }
        queryClient.setQueriesData<SessionListEntry[]>(
          { queryKey, exact: true },
          (current) =>
            current?.map((session) =>
              session.id === sessionId && session.pinned === pinned
                ? { ...session, pinned: prevPinned }
                : session,
            ),
        );
      }
      if (context?.previousDetailPinned !== undefined) {
        const prevPinned = context.previousDetailPinned;
        queryClient.setQueryData<SessionDetail>(context.sessionKey, (current) =>
          current && current.session.pinned === pinned
            ? { ...current, session: { ...current.session, pinned: prevPinned } }
            : current,
        );
      }
    },
    onSuccess: (session, _variables, context) => {
      if (
        context?.requestToken !== latestRequestBySessionRef.current.get(session.id) ||
        hasOtherInFlightForSession(session.id)
      ) {
        return;
      }
      queryClient.setQueriesData<SessionListEntry[]>(
        { queryKey: ["talos"], predicate: sessionsPredicate },
        (current) =>
          current?.map((entry) =>
            entry.id === session.id ? { ...entry, pinned: session.pinned } : entry,
          ),
      );
      queryClient.setQueryData<SessionDetail>(
        chatKeys.session(connectionKey, session.id),
        (current) => (current ? { ...current, session } : current),
      );
    },
    onSettled: async (_data, _error, { sessionId }) => {
      if (queryClient.isMutating({ mutationKey }) > 1) {
        return;
      }
      await queryClient.invalidateQueries({ queryKey: ["talos"], predicate: sessionsPredicate });
      await queryClient.invalidateQueries({ queryKey: chatKeys.session(connectionKey, sessionId) });
    },
  });
}
