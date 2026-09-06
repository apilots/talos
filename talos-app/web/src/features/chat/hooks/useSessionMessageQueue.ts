import { useCallback, useEffect, useRef, useState } from "react";
import type {
  AgentUserInput,
  MessageQueue,
  MessageQueueChangedNotification,
  MessageQueueAddRequest,
  MessageQueueUpdateRequest,
} from "@talos/sdk-browser";

import { MessageQueueSync } from "../message-queue-sync";
import { getTalosClient } from "@/shared/api/client";
import { userVisibleErrorMessage } from "@/shared/i18n/apiErrorMessage";

export function useSessionMessageQueue(sessionId: string) {
  const [queue, setQueue] = useState<MessageQueue>();
  const [error, setError] = useState<string>();
  const syncRef = useRef<MessageQueueSync | undefined>(undefined);

  useEffect(() => {
    setQueue(undefined);
    setError(undefined);
  }, [sessionId]);

  // Queue revisions share the Session event connection; this hook owns only
  // queue reconciliation and mutations, not a second transport lifecycle.
  const onDisconnect = useCallback(() => {
    syncRef.current?.clear();
    syncRef.current = undefined;
  }, []);
  useEffect(() => onDisconnect, [onDisconnect, sessionId]);

  const onOpen = useCallback((onError: (error: unknown) => void) => {
    const reportError = (error: unknown) => {
      if (syncRef.current !== sync) return;
      setError(userVisibleErrorMessage(error, "Message queue reconciliation failed."));
      onError(error);
    };
    const sync = new MessageQueueSync({
      list: () => getTalosClient().messageQueue.list({ sessionId }),
      onChange: (nextQueue) => {
        setQueue(nextQueue);
        if (nextQueue) setError(undefined);
      },
      onError: reportError,
    });
    syncRef.current?.clear();
    syncRef.current = sync;
    void sync.refresh().catch(reportError);
  }, [sessionId]);

  const onMessageQueueChanged = useCallback((change: MessageQueueChangedNotification) => {
    if (change.sessionId === sessionId) {
      syncRef.current?.observe(change.revision);
    }
  }, [sessionId]);

  const applyMutation = useCallback(async (operation: () => Promise<MessageQueue>) => {
    try {
      const result = await operation();
      const sync = syncRef.current;
      if (sync) {
        sync.apply(result);
      } else {
        setQueue((current) =>
          !current || result.revision >= current.revision ? result : current,
        );
      }
      setError(undefined);
      return result;
    } catch (operationError) {
      setError(userVisibleErrorMessage(operationError, "Message queue operation failed."));
      throw operationError;
    }
  }, []);

  return {
    onOpen,
    onMessageQueueChanged,
    onDisconnect,
    queue,
    error,
    add: (input: AgentUserInput) =>
      applyMutation(() => getTalosClient().messageQueue.add({ sessionId, input } satisfies MessageQueueAddRequest)),
    update: (messageId: string, input: AgentUserInput) =>
      applyMutation(() => getTalosClient().messageQueue.update({
        sessionId,
        messageId,
        input,
      } satisfies MessageQueueUpdateRequest)),
    delete: (messageId: string) =>
      applyMutation(() => getTalosClient().messageQueue.delete({ sessionId, messageId })),
    reorder: (messageIds: string[]) =>
      applyMutation(() => getTalosClient().messageQueue.reorder({ sessionId, messageIds })),
    steer: async (messageId: string, turnId: string) => {
      try {
        const response = await getTalosClient().turns.input({
          identity: { sessionId, turnId },
          input: { type: "queuedMessage", messageId },
          clientRequestId: null,
        });
        await syncRef.current?.refresh();
        setError(undefined);
        return response;
      } catch (operationError) {
        setError(userVisibleErrorMessage(operationError, "Failed to steer queued message."));
        throw operationError;
      }
    },
    start: async (messageId?: string) => {
      try {
        const response = await getTalosClient().messageQueue.start({
          sessionId,
          messageId: messageId ?? null,
        });
        await syncRef.current?.refresh();
        setError(undefined);
        return response;
      } catch (operationError) {
        setError(userVisibleErrorMessage(operationError, "Failed to start queued message."));
        throw operationError;
      }
    },
  };
}
