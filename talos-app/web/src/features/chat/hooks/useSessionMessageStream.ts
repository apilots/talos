import { useCallback, useEffect, useRef } from "react";

import { sendMessageStream } from "../api";
import { applyStreamEvent } from "../stream-events";

export function useSessionMessageStream(sessionId: string) {
  const abortRef = useRef<AbortController | null>(null);

  useEffect(() => {
    return () => {
      abortRef.current?.abort();
    };
  }, []);

  const streamMessage = useCallback(
    async (text: string) => {
      abortRef.current?.abort();
      const controller = new AbortController();
      abortRef.current = controller;

      try {
        await sendMessageStream(
          sessionId,
          text,
          (event) => applyStreamEvent(sessionId, event),
          controller.signal,
        );
      } finally {
        if (abortRef.current === controller) {
          abortRef.current = null;
        }
      }
    },
    [sessionId],
  );

  const cancelStream = useCallback(() => {
    abortRef.current?.abort();
    abortRef.current = null;
  }, []);

  const hasActiveConnection = useCallback(() => abortRef.current !== null, []);

  return {
    streamMessage,
    cancelStream,
    hasActiveConnection,
  };
}
