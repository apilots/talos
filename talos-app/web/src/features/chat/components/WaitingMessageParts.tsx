import type { ReactNode } from "react";
import type { AgentUserContent, AgentUserInput, QueuedMessage } from "@talos/sdk-browser";

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
  return message.input.content
    .filter((item): item is Extract<AgentUserContent, { type: "text" }> => item.type === "text")
    .map((item) => item.text)
    .join("\n");
}

export function replaceQueuedMessageText(
  input: AgentUserInput,
  text: string,
): AgentUserInput {
  const nonText = input.content.filter((item) => item.type !== "text");
  return {
    content: text.trim() ? [{ type: "text", text: text.trim() }, ...nonText] : nonText,
  };
}

export function queuedMessageSummary(
  message: QueuedMessage,
  t: (key: string, options?: Record<string, unknown>) => string,
) {
  const parts = message.input.content.flatMap((item) => {
    switch (item.type) {
      case "text":
        return item.text.trim() ? [item.text.trim()] : [];
      case "file":
        return [t("queuedAttachments", { count: 1 })];
      case "skillSelection":
        return [`$${item.name}`];
      case "audio":
      case "video":
        return [t("queuedMedia")];
      case "userActionResponse":
        return [];
    }
  });
  return parts.join(" · ") || t("queuedMessage");
}
