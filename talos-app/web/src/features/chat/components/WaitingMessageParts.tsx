import type { ReactNode } from "react";
import type { QueuedMessage, TalosInputItem } from "@talos/sdk-browser";

export function WaitingMessageMenuButton({
  disabled,
  icon,
  label,
  onClick,
}: {
  disabled?: boolean;
  icon: ReactNode;
  label: string;
  onClick: () => void;
}) {
  return (
    <button
      className="flex w-full items-center gap-2 rounded px-2 py-1.5 text-left text-[12px] text-ink/80 hover:bg-muted/10 disabled:text-muted/40"
      disabled={disabled}
      onClick={onClick}
      type="button"
    >
      {icon}
      {label}
    </button>
  );
}

export function queuedMessageText(message: QueuedMessage) {
  return message.input
    .filter((item): item is Extract<TalosInputItem, { type: "text" }> => item.type === "text")
    .map((item) => item.text)
    .join("\n");
}

export function replaceQueuedMessageText(
  input: TalosInputItem[],
  text: string,
): TalosInputItem[] {
  const nonText = input.filter((item) => item.type !== "text");
  return text.trim() ? [{ type: "text", text: text.trim() }, ...nonText] : nonText;
}

export function queuedMessageSummary(
  message: QueuedMessage,
  t: (key: string, options?: Record<string, unknown>) => string,
) {
  const parts = message.input.flatMap((item) => {
    switch (item.type) {
      case "text":
        return item.text.trim() ? [item.text.trim()] : [];
      case "files":
        return [t("queuedAttachments", { count: item.fileIds.length })];
      case "skill_selection":
        return [`$${item.name}`];
      case "voice":
      case "video":
        return [t("queuedMedia")];
    }
  });
  return parts.join(" · ") || t("queuedMessage");
}
