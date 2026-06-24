import { useCallback, useEffect, useRef } from "react";

import { retryTimelineEvents, sendTimelineEvents, subscribeTimelineEventsForMessage } from "../api";
import { applyStreamEvent } from "../stream-events";
import type { RetryTurnRequest, StartTurnRequest } from "@talos/sdk-browser";

export function useSessionMessageStream(sessionId: string) {
  const abortRef = useRef<AbortController | null>(null);

  useEffect(() => {
    return () => {
      abortRef.current?.abort();
    };
  }, []);

  const eventMessage = useCallback(
    async (
      request: StartTurnRequest,
      onAcceptedMessageId?: (messageId: string) => void,
    ) => {
      abortRef.current?.abort();
      const controller = new AbortController();
      abortRef.current = controller;

      try {
        await sendTimelineEvents(
          request,
          (event) => applyStreamEvent(sessionId, event),
          controller.signal,
          onAcceptedMessageId,
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
    async (messageId: string, afterEventId?: string) => {
      abortRef.current?.abort();
      const controller = new AbortController();
      abortRef.current = controller;

      try {
        await subscribeTimelineEventsForMessage(
          sessionId,
          messageId,
          (event) => applyStreamEvent(sessionId, event),
          controller.signal,
          afterEventId,
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
      onAcceptedMessageId,
    }: {
      clientMessageId: string;
      retryOfMessageId: string;
      text: string;
      onAcceptedMessageId?: (messageId: string) => void;
    }) => {
      abortRef.current?.abort();
      const controller = new AbortController();
      abortRef.current = controller;

      try {
        await retryTimelineEvents(
          {
            session_id: sessionId,
            client_message_id: clientMessageId,
            retry_of_message_id: retryOfMessageId,
            input: [{ type: "text", text }],
          } satisfies RetryTurnRequest,
          (event) => applyStreamEvent(sessionId, event),
          controller.signal,
          onAcceptedMessageId,
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
