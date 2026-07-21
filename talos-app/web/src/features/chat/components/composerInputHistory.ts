import type { SessionInputHistory } from "@/shared/types/ui";

const EMPTY_INPUT_HISTORY_ITEMS: Array<{ key: string; text: string }> = [];

export function buildInputHistoryNavigationItems(inputHistory: SessionInputHistory | undefined) {
  if (!inputHistory) {
    return EMPTY_INPUT_HISTORY_ITEMS;
  }

  const items = inputHistory.committed.map((item) => ({
    key: `${item.messageId}:${item.itemIndex}`,
    text: item.text,
  }));
  const pendingInput = inputHistory.pendingInput;
  const pendingCommitted =
    Boolean(pendingInput?.serverMessageId) &&
    inputHistory.committed.some((item) => item.messageId === pendingInput?.serverMessageId);

  if (
    pendingInput &&
    (pendingInput.phase === "local" || pendingInput.phase === "accepted") &&
    !pendingCommitted
  ) {
    items.push({
      key: [
        "pending",
        pendingInput.clientMessageId,
        pendingInput.serverMessageId ?? "",
        pendingInput.phase,
      ].join(":"),
      text: pendingInput.text,
    });
  }

  return items;
}
