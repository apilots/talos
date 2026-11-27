import { useMemo } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { chatKeys, useConnectionKey } from "./useChatQueries";

export function useSessionQueryInvalidation(sessionId: string) {
  const connectionKey = useConnectionKey();
  const queryClient = useQueryClient();

  const invalidateSessionQueries = useMemo(() => {
    let requested = false;
    let inFlight: Promise<void> | undefined;
    async function refresh() {
      while (requested) {
        requested = false;
        await queryClient.invalidateQueries({ queryKey: ["talos", connectionKey, "automation"] });
        await queryClient.invalidateQueries({ predicate: (query) => query.queryKey[0] === "talos" && query.queryKey.includes("sessions") });
        await Promise.all([
          chatKeys.session(connectionKey, sessionId),
          chatKeys.activeTurn(connectionKey, sessionId),
          chatKeys.sessionHistory(connectionKey, sessionId),
        ].map((queryKey) => queryClient.invalidateQueries({ queryKey })));
      }
    }
    return () => {
      // Serialize notification bursts and retain changes arriving during a read.
      requested = true;
      inFlight ??= refresh().finally(() => { inFlight = undefined; });
      return inFlight;
    };
  }, [connectionKey, queryClient, sessionId]);

  return { connectionKey, invalidateSessionQueries, queryClient };
}
