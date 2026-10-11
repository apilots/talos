import type { AppProtocol } from "@talos/sdk-browser";
import type { PendingInput, SessionInputHistory, SessionInputHistoryItem } from "@/shared/types/ui";

export function buildSessionInputHistory(
  history: AppProtocol.SessionHistoryResponse | undefined,
  pendingInput: PendingInput | undefined,
): SessionInputHistory {
  const committed: SessionInputHistoryItem[] = [];

  let itemIndex = 0;
  (history?.data ?? []).forEach((turn) => {
    turn.result.result.messages.forEach((message) => {
      if (message.role !== "user") {
        return;
      }
      const text: string[] = [];
      const fileIds: string[] = [];
      message.blocks.forEach((block) => {
        if (block.content.type !== "userContent") {
          return;
        }
        if (block.content.data.type === "text") {
          text.push(block.content.data.text);
        } else if (block.content.data.type === "file") {
          fileIds.push(block.content.data.fileId);
        }
      });
      if (text.length > 0 || fileIds.length > 0) {
        committed.push({
          messageId: message.messageId,
          itemIndex,
          text: text.join("\n"),
          fileIds,
        });
        itemIndex += 1;
      }
    });
  });

  return {
    committed,
    pendingInput,
  };
}

export function sessionInputHistoryHasMessage(
  inputHistory: SessionInputHistory,
  messageId: string | undefined,
) {
  if (!messageId) {
    return false;
  }
  return inputHistory.committed.some((item) => item.messageId === messageId);
}
