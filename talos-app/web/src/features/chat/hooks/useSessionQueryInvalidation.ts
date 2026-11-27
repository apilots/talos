import { useQueryClient } from "@tanstack/react-query";
import { chatKeys, useConnectionKey } from "./useChatQueries";

export function useSessionQueryInvalidation(sessionId: string) {
  const connectionKey = useConnectionKey();
  const queryClient = useQueryClient();
  const sessionKeys = [
    chatKeys.session(connectionKey, sessionId),
    chatKeys.activeTurn(connectionKey, sessionId),
    chatKeys.sessionHistory(connectionKey, sessionId),
  ];

  async function invalidateSessionQueries() {
    await queryClient.invalidateQueries({ queryKey: ["talos", connectionKey, "automation"] });
    await queryClient.invalidateQueries({ predicate: (query) => query.queryKey[0] === "talos" && query.queryKey.includes("sessions") });
    await Promise.all(sessionKeys.map((queryKey) => queryClient.invalidateQueries({ queryKey })));
  }

  return { connectionKey, invalidateSessionQueries, queryClient };
}
