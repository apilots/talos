import { useEffect, useId } from "react";
import { AutomationTaskRow } from "@/features/automation/AutomationTaskRow";
import type { SessionAutomationController } from "@/features/automation/useSessionAutomation";
import { DropdownMenu, DropdownMenuContent, DropdownMenuTrigger } from "@/shared/ui/DropdownMenu";
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
  controller?: MessageQueueController;
  automation?: SessionAutomationController;
}

export function WaitingMessages({ controller, automation }: WaitingMessagesProps) {
  const { activeTurnId, error, queue } = controller ?? {};
  const listId = useId();
  const [expanded, setExpanded] = useState(true);
  const { t } = useTranslation("chat");
  const [menuMessageId, setMenuMessageId] = useState<string>();
  const [editingMessageId, setEditingMessageId] = useState<string>();
  const [editText, setEditText] = useState("");
  const steeringMessageIdsRef = useRef(new Set<string>());
  const [steeringMessageIds, setSteeringMessageIds] = useState(new Set<string>());
  const allMessages = queue?.messages ?? [];
  const tasks = automation?.tasks ?? [];
  useEffect(() => { if (automation?.readRequest) setExpanded(true); }, [automation?.readRequest]);
  const messages = allMessages.filter((message) => message.source?.type !== "loop");
  const orphanLoops = allMessages.filter((message) => message.source?.type === "loop" && !tasks.some((task) => task.type === "loop" && task.value.id === (message.source.type === "loop" ? message.source.loopId : undefined)));

  if (allMessages.length === 0 && tasks.length === 0 && !error && !automation?.error && !automation?.readRequest) {
    return null;
  }

  const move = (index: number, offset: number) => {
    const target = index + offset;
    if (target < 0 || target >= messages.length) {
      return;
    }
    const messageIds = allMessages.map((message) => message.id);
    const sourceIndex = messageIds.indexOf(messages[index]!.id);
    const targetIndex = messageIds.indexOf(messages[target]!.id);
    [messageIds[sourceIndex], messageIds[targetIndex]] = [messageIds[targetIndex]!, messageIds[sourceIndex]!];
    setMenuMessageId(undefined);
    void controller?.reorder(messageIds).catch(() => undefined);
  };

  const steer = (messageId: string, turnId: string) => {
    if (steeringMessageIdsRef.current.has(messageId)) {
      return;
    }

    const pending = new Set(steeringMessageIdsRef.current).add(messageId);
    steeringMessageIdsRef.current = pending;
    setSteeringMessageIds(pending);
    void controller?.steer(messageId, turnId)
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
      <button aria-controls={listId} aria-expanded={expanded} className="mb-1 flex w-full items-center justify-between rounded px-1 py-1 text-[11px] text-muted focus-visible:ring-2 focus-visible:ring-brand/25" onClick={() => setExpanded((value) => !value)} type="button">
        <span className="flex items-center gap-1">{expanded ? <ChevronDown size={12} /> : <ChevronUp size={12} />}{tasks.length ? t("automationTasksAndMessages") : t("waitingMessages")}</span>
        {allMessages.length ? <span>{allMessages.length}/{queue?.maxMessages}</span> : null}
      </button>
      <div id={listId} hidden={!expanded} className="max-h-[min(28vh,16rem)] space-y-0.5 overflow-y-auto overscroll-contain">
        {automation?.readRequest && !tasks.some((task) => task.type === automation.readRequest!.mode) ? <p className="px-1.5 py-2 text-xs text-muted">{t(automation.readRequest.mode === "goal" ? "noSessionGoal" : "noSessionLoops")}</p> : null}
        {tasks.map((task) => <AutomationTaskRow key={`${task.type}-${task.value.id}`} task={task} activeTurn={automation?.activeTurn} queuedLoop={task.type === "loop" && allMessages.some((message) => message.source?.type === "loop" && message.source.loopId === task.value.id)} pending={automation?.pending} onAction={automation!.execute} onEdit={automation!.setEditing} />)}
        {orphanLoops.map((message) => <div key={message.id} className="px-1.5 py-2 text-[13px] text-muted">Loop · {queuedMessageSummary(message, t)}</div>)}
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
                      void controller?.update(message.id, input)
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

              {!editing && activeTurnId && controller?.canSteer !== false && steerable ? (
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
              {!editing && !activeTurnId && controller?.canStart !== false ? (
                <button
                  aria-label={t("startQueuedMessage")}
                  className="shrink-0 p-1 text-muted transition hover:text-ink"
                  onClick={() => void controller?.start(message.id).catch(() => undefined)}
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
                  onClick={() => void controller?.delete(message.id).catch(() => undefined)}
                  title={t("deleteQueuedMessage")}
                  type="button"
                >
                  <Trash2 size={14} />
                </button>
              ) : null}
              {!editing ? <DropdownMenu open={menuMessageId === message.id} onOpenChange={(open) => setMenuMessageId(open ? message.id : undefined)}>
                <DropdownMenuTrigger asChild>
                <button
                  aria-label={t("moreQueuedMessageActions")}
                  className="shrink-0 p-1 text-muted/75 transition hover:text-ink"
                  title={t("moreQueuedMessageActions")}
                  type="button"
                >
                  <MoreHorizontal size={14} />
                </button>
                </DropdownMenuTrigger>
                <DropdownMenuContent align="end">
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
                </DropdownMenuContent>
              </DropdownMenu> : null}

            </div>
          );
        })}
      </div>
      {automation?.error ? <p role="alert" className="px-1 pt-1 text-[12px] text-danger">{automation.error}</p> : null}
      {error ? <p className="px-1 pt-1 text-[12px] text-danger">{error}</p> : null}
    </div>
  );
}
