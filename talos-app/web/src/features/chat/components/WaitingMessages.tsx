import { useRef, useState } from "react";
import { ChevronDown, ChevronUp, LoaderCircle, MoreHorizontal, Pencil, Play, Trash2 } from "lucide-react";
import { useTranslation } from "react-i18next";
import type { MessageQueueController } from "./ChatComposer.types";
import {
  queuedMessageSummary,
  queuedMessageText,
  replaceQueuedMessageText,
  WaitingMessageMenuButton,
} from "./WaitingMessageParts";

interface WaitingMessagesProps {
  controller: MessageQueueController;
}

export function WaitingMessages({ controller }: WaitingMessagesProps) {
  const { activeTurnId, error, queue } = controller;
  const { t } = useTranslation("chat");
  const [menuMessageId, setMenuMessageId] = useState<string>();
  const [editingMessageId, setEditingMessageId] = useState<string>();
  const [editText, setEditText] = useState("");
  const steeringMessageIdsRef = useRef(new Set<string>());
  const [steeringMessageIds, setSteeringMessageIds] = useState(new Set<string>());
  const messages = queue?.messages ?? [];

  if (messages.length === 0 && !error) {
    return null;
  }

  const move = (index: number, offset: number) => {
    const target = index + offset;
    if (target < 0 || target >= messages.length) {
      return;
    }
    const messageIds = messages.map((message) => message.id);
    [messageIds[index], messageIds[target]] = [messageIds[target]!, messageIds[index]!];
    setMenuMessageId(undefined);
    void controller.reorder(messageIds).catch(() => undefined);
  };

  const steer = (messageId: string, turnId: string) => {
    if (steeringMessageIdsRef.current.has(messageId)) {
      return;
    }

    const pending = new Set(steeringMessageIdsRef.current).add(messageId);
    steeringMessageIdsRef.current = pending;
    setSteeringMessageIds(pending);
    void controller.steer(messageId, turnId)
      .catch(() => undefined)
      .finally(() => {
        const completed = new Set(steeringMessageIdsRef.current);
        completed.delete(messageId);
        steeringMessageIdsRef.current = completed;
        setSteeringMessageIds(completed);
      });
  };

  return (
    <div className="border-b border-border/50 px-4 pb-2 pt-3" data-testid="waiting-messages">
      {messages.length > 0 ? (
        <div className="mb-1 flex items-center justify-between px-1 text-[11px] text-muted">
          <span>{t("waitingMessages")}</span>
          <span>{messages.length}/{queue?.maxMessages}</span>
        </div>
      ) : null}
      <div className="space-y-0.5">
        {messages.map((message, index) => {
          const editing = editingMessageId === message.id;
          const steerable = message.input.content.every(
            (item) => item.type === "text" || item.type === "file",
          );
          const steering = steeringMessageIds.has(message.id);
          return (
            <div
              className={`group relative flex min-h-9 items-center gap-2 rounded-md px-1.5 py-1${steering ? "" : " hover:bg-hover"}`}
              key={message.id}
            >
              {editing ? (
                <input
                  autoFocus
                  className="form-control-focus min-w-0 flex-1 rounded-md border border-border/60 bg-transparent px-2 py-1 text-[13px]"
                  onChange={(event) => setEditText(event.target.value)}
                  onKeyDown={(event) => {
                    if (event.key === "Escape") {
                      setEditingMessageId(undefined);
                    }
                    if (event.key === "Enter") {
                      const input = replaceQueuedMessageText(message.input, editText);
                      void controller.update(message.id, input)
                        .then(() => setEditingMessageId(undefined))
                        .catch(() => undefined);
                    }
                  }}
                  value={editText}
                />
              ) : (
                <p className="min-w-0 flex-1 truncate text-[13px] text-ink/85" title={queuedMessageSummary(message, t)}>
                  {queuedMessageSummary(message, t)}
                </p>
              )}

              {!editing && activeTurnId && steerable ? (
                <button
                  className="flex shrink-0 items-center gap-1 px-1.5 py-1 text-[12px] text-muted transition hover:text-ink disabled:cursor-not-allowed disabled:opacity-50"
                  disabled={steering}
                  onClick={() => steer(message.id, activeTurnId)}
                  type="button"
                >
                  {steering ? <LoaderCircle className="animate-spin" size={12} /> : null}
                  {t("steerQueuedMessage")}
                </button>
              ) : null}
              {!editing && !activeTurnId ? (
                <button
                  aria-label={t("startQueuedMessage")}
                  className="shrink-0 p-1 text-muted transition hover:text-ink"
                  onClick={() => void controller.start(message.id).catch(() => undefined)}
                  title={t("startQueuedMessage")}
                  type="button"
                >
                  <Play size={14} />
                </button>
              ) : null}
              {!editing ? (
                <button
                  aria-label={t("deleteQueuedMessage")}
                  className="shrink-0 p-1 text-muted/75 transition hover:text-ink"
                  onClick={() => void controller.delete(message.id).catch(() => undefined)}
                  title={t("deleteQueuedMessage")}
                  type="button"
                >
                  <Trash2 size={14} />
                </button>
              ) : null}
              {!editing ? (
                <button
                  aria-label={t("moreQueuedMessageActions")}
                  className="shrink-0 p-1 text-muted/75 transition hover:text-ink"
                  onClick={() => setMenuMessageId(menuMessageId === message.id ? undefined : message.id)}
                  title={t("moreQueuedMessageActions")}
                  type="button"
                >
                  <MoreHorizontal size={14} />
                </button>
              ) : null}

              {menuMessageId === message.id ? (
                <div className="absolute right-0 top-8 z-30 min-w-36 rounded-md border border-border/70 bg-card p-1 shadow-lg">
                  <WaitingMessageMenuButton
                    icon={<Pencil size={13} />}
                    label={t("editQueuedMessage")}
                    onClick={() => {
                      setEditText(queuedMessageText(message));
                      setEditingMessageId(message.id);
                      setMenuMessageId(undefined);
                    }}
                  />
                  <WaitingMessageMenuButton
                    disabled={index === 0}
                    icon={<ChevronUp size={13} />}
                    label={t("moveQueuedMessageUp")}
                    onClick={() => move(index, -1)}
                  />
                  <WaitingMessageMenuButton
                    disabled={index === messages.length - 1}
                    icon={<ChevronDown size={13} />}
                    label={t("moveQueuedMessageDown")}
                    onClick={() => move(index, 1)}
                  />
                </div>
              ) : null}
            </div>
          );
        })}
      </div>
      {error ? <p className="px-1 pt-1 text-[12px] text-danger">{error}</p> : null}
    </div>
  );
}
