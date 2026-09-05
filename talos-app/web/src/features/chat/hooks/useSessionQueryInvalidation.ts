import { useQueryClient } from "@tanstack/react-query";
import { chatKeys, useConnectionKey } from "./useChatQueries";

export function useSessionQueryInvalidation(sessionId: string) {
  const connectionKey = useConnectionKey();
  const queryClient = useQueryClient();

  async function invalidateSessionQueries() {
    await Promise.all([
      chatKeys.sessions(connectionKey),
      chatKeys.session(connectionKey, sessionId),
      chatKeys.activeTurn(connectionKey, sessionId),
      chatKeys.sessionHistory(connectionKey, sessionId),
    ].map((queryKey) => queryClient.invalidateQueries({ queryKey })));
  }

  return { connectionKey, invalidateSessionQueries, queryClient };
}
