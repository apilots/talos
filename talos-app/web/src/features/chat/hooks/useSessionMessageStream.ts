import { useCallback, useEffect, useRef } from "react";

import { retryTimelineTurn, startTimelineTurn } from "../api";
import { useAppStore } from "@/shared/store/app-store";
import type { ChatUserInput } from "@/shared/types/ui";
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
        const messageId = await startTimelineTurn(request, onAcceptedMessageId);
        await waitForMessageTerminal(sessionId, messageId, controller.signal);
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

  const retryMessage = useCallback(
    async ({
      clientMessageId,
      retryOfMessageId,
      input,
      onAcceptedMessageId,
    }: {
      clientMessageId: string;
      retryOfMessageId: string;
      input: ChatUserInput;
      onAcceptedMessageId?: (messageId: string) => void;
    }) => {
      abortRef.current?.abort();
      const controller = new AbortController();
      abortRef.current = controller;

      try {
        const messageId = await retryTimelineTurn(
          {
            session_id: sessionId,
            client_message_id: clientMessageId,
            retry_of_message_id: retryOfMessageId,
            input: [
              ...(input.text.trim() ? [{ type: "text" as const, text: input.text }] : []),
              ...(input.fileIds.length > 0
                ? [{ type: "files" as const, fileIds: input.fileIds }]
                : []),
            ],
          } satisfies RetryTurnRequest,
          onAcceptedMessageId,
        );
        await waitForMessageTerminal(sessionId, messageId, controller.signal);
      } finally {
        if (abortRef.current === controller) {
          abortRef.current = null;
        }
      }
    },
    [sessionId],
  );

  return {
    eventMessage,
    cancelStream,
    retryMessage,
  };
}

function waitForMessageTerminal(
  sessionId: string,
  messageId: string,
  signal: AbortSignal,
): Promise<void> {
  return new Promise((resolve, reject) => {
    let settled = false;
    let unsubscribe: () => void = () => undefined;
    const onAbort = () => finish(new DOMException("Aborted", "AbortError"));
    const finish = (error?: Error) => {
      if (settled) {
        return;
      }
      settled = true;
      unsubscribe();
      signal.removeEventListener("abort", onAbort);
      if (error) {
        reject(error);
      } else {
        resolve();
      }
    };
    const isTerminal = () => {
      const stream = useAppStore.getState().streams[sessionId];
      return stream?.activeMessageId === messageId && !stream.isStreaming;
    };
    unsubscribe = useAppStore.subscribe(() => {
      if (isTerminal()) {
        finish();
      }
    });
    signal.addEventListener("abort", onAbort, { once: true });
    if (signal.aborted) {
      onAbort();
    } else if (isTerminal()) {
      finish();
    }
  });
}
