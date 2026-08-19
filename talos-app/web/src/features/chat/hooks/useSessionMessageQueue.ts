import { useCallback, useEffect, useRef, useState } from "react";
import type {
  MessageQueue,
  MessageQueueAddRequest,
  MessageQueueUpdateRequest,
  TalosInputItem,
} from "@talos/sdk-browser";

import { MessageQueueSync } from "../message-queue-sync";
import { getTalosClient } from "@/shared/api/client";
import { userVisibleErrorMessage } from "@/shared/i18n/apiErrorMessage";

const RECONNECT_DELAYS_MS = [500, 1_000, 2_000, 4_000, 8_000, 12_000] as const;

export function useSessionMessageQueue(
  sessionId: string,
  enabled: boolean,
  onQueueChanged?: () => void,
) {
  const [queue, setQueue] = useState<MessageQueue>();
  const [error, setError] = useState<string>();
  const syncRef = useRef<MessageQueueSync | undefined>(undefined);
  const onQueueChangedRef = useRef(onQueueChanged);
  onQueueChangedRef.current = onQueueChanged;

  useEffect(() => {
    setQueue(undefined);
    setError(undefined);
  }, [sessionId]);

  useEffect(() => {
    if (!enabled || !sessionId) {
      syncRef.current?.clear();
      syncRef.current = undefined;
      return;
    }

    let disposed = false;
    let activeController: AbortController | undefined;
    const sync = new MessageQueueSync({
      list: () => getTalosClient().messageQueue.list({ sessionId }),
      onChange: (nextQueue) => {
        setQueue(nextQueue);
        if (nextQueue) {
          setError(undefined);
        }
      },
      onError: () => activeController?.abort(),
    });
    syncRef.current = sync;

    const run = async () => {
      for (let attempt = 0; !disposed; attempt += 1) {
        const client = getTalosClient();
        const controller = new AbortController();
        activeController = controller;
        let resolveOpen: () => void = () => undefined;
        const opened = new Promise<void>((resolve) => { resolveOpen = resolve; });
        const stream = client.turns.events(sessionId, undefined, () => undefined, controller.signal, {
          persistent: true,
          onOpen: resolveOpen,
          onMessageQueueChanged: (change) => {
            if (change.sessionId === sessionId) {
              sync.observe(change.revision);
              onQueueChangedRef.current?.();
            }
          },
        });

        try {
          await Promise.race([
            opened,
            stream.then(() => Promise.reject(new Error("Session event subscription closed."))),
          ]);
          await sync.refresh();
          setError(undefined);
          await stream;
          throw new Error("Session event subscription closed.");
        } catch (subscriptionError) {
          controller.abort();
          void stream.catch(() => undefined);
          if (disposed) {
            return;
          }
          sync.clear();
          setError(userVisibleErrorMessage(subscriptionError, "Message queue subscription failed."));
          await delay(RECONNECT_DELAYS_MS[Math.min(attempt, RECONNECT_DELAYS_MS.length - 1)]);
        }
      }
    };

    void run();
    return () => {
      disposed = true;
      activeController?.abort();
      sync.clear();
      if (syncRef.current === sync) {
        syncRef.current = undefined;
      }
    };
  }, [enabled, sessionId]);

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
    queue,
    error,
    add: (input: TalosInputItem[]) =>
      applyMutation(() => getTalosClient().messageQueue.add({ sessionId, input } satisfies MessageQueueAddRequest)),
    update: (messageId: string, input: TalosInputItem[]) =>
      applyMutation(() => getTalosClient().messageQueue.update({
        sessionId,
        messageId,
        input,
      } satisfies MessageQueueUpdateRequest)),
    delete: (messageId: string) =>
      applyMutation(() => getTalosClient().messageQueue.delete({ sessionId, messageId })),
    reorder: (messageIds: string[]) =>
      applyMutation(() => getTalosClient().messageQueue.reorder({ sessionId, messageIds })),
    steer: (messageId: string, turnId: string) =>
      applyMutation(() => getTalosClient().messageQueue.steer({ sessionId, messageId, turnId })),
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

function delay(duration: number) {
  return new Promise<void>((resolve) => window.setTimeout(resolve, duration));
}
