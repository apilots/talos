import { useCallback, useEffect, useRef } from "react";

import { retryMessageEvents, sendMessageEvents, subscribeSessionEventsForMessage } from "../api";
import { applyStreamEvent } from "../stream-events";

export function useSessionMessageStream(sessionId: string) {
  const abortRef = useRef<AbortController | null>(null);

  useEffect(() => {
    return () => {
      abortRef.current?.abort();
    };
  }, []);

  const eventMessage = useCallback(
    async (text: string) => {
      abortRef.current?.abort();
      const controller = new AbortController();
      abortRef.current = controller;

      try {
        await sendMessageEvents(
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

  const reconnectMessage = useCallback(
    async (messageId: string) => {
      abortRef.current?.abort();
      const controller = new AbortController();
      abortRef.current = controller;

      try {
        await subscribeSessionEventsForMessage(
          sessionId,
          messageId,
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

  const retryMessage = useCallback(
    async ({
      clientMessageId,
      retryOfMessageId,
      text,
    }: {
      clientMessageId: string;
      retryOfMessageId: string;
      text: string;
    }) => {
      abortRef.current?.abort();
      const controller = new AbortController();
      abortRef.current = controller;

      try {
        await retryMessageEvents(
          sessionId,
          {
            client_message_id: clientMessageId,
            retry_of_message_id: retryOfMessageId,
            input: [{ type: "text", text }],
          },
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

  const hasActiveConnection = useCallback(() => abortRef.current !== null, []);

  return {
    eventMessage,
    cancelStream,
    reconnectMessage,
    retryMessage,
    hasActiveConnection,
  };
}
