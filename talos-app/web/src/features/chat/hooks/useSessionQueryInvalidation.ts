import { useQueryClient } from "@tanstack/react-query";

import { chatKeys, useConnectionKey } from "./useChatQueries";

export function useSessionQueryInvalidation(sessionId: string) {
  const connectionKey = useConnectionKey();
  const queryClient = useQueryClient();

  async function invalidateSessionQueries() {
    await queryClient.invalidateQueries({ queryKey: chatKeys.sessions(connectionKey) });
    await queryClient.invalidateQueries({ queryKey: chatKeys.session(connectionKey, sessionId) });
    await queryClient.invalidateQueries({ queryKey: chatKeys.turns(connectionKey, sessionId) });
  }

  return {
    connectionKey,
    invalidateSessionQueries,
    queryClient,
  };
}
