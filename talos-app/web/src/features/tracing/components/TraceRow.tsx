import type { MouseEventHandler, ReactNode } from "react";
import {
  Brain,
  FileText,
  Layers,
  MessageSquareText,
  Sparkles,
  SquareTerminal,
  type LucideIcon,
} from "lucide-react";
import { cn } from "@/shared/lib/cn";
import { selectionClasses } from "@/shared/ui/controlStyles";

const icons: Record<string, LucideIcon> = {
  reasoning: Brain,
  assistant: MessageSquareText,
  tool: SquareTerminal,
  user: MessageSquareText,
  llm: Sparkles,
  assistantText: MessageSquareText,
  userContent: MessageSquareText,
  toolCall: SquareTerminal,
  toolResult: SquareTerminal,
  context: Layers,
  system: Layers,
  executionSummary: Layers,
};

export function TraceBadge({
  kind,
  children,
}: {
  kind: string;
  children: ReactNode;
}) {
  const Icon = icons[kind] ?? FileText;
  return (
    <span className="tracing-tag" data-kind={kind}>
      <Icon aria-hidden="true" size={12} />
      <span>{children}</span>
    </span>
  );
}

/** One grid and interaction treatment for every execution-ledger row. */
export function TraceRow({
  kind,
  tag,
  title,
  preview,
  meta,
  status,
  time = "—",
  selected = false,
  onClick,
}: {
  kind: string;
  tag: string;
  title: ReactNode;
  preview: ReactNode;
  meta?: ReactNode;
  status?: string;
  time?: ReactNode;
  selected?: boolean;
  onClick: MouseEventHandler<HTMLButtonElement>;
}) {
  return (
    <button
      type="button"
      className={cn(
        "tracing-row",
        selected ? selectionClasses.selected : selectionClasses.idle,
      )}
      aria-pressed={selected}
      onClick={onClick}
    >
      <TraceBadge kind={kind}>{tag}</TraceBadge>
      <span className="tracing-row-main">
        {title ? <strong>{title}</strong> : null}
        <span className="tracing-row-text">{preview}</span>
        {meta ? <small data-status={status}>{meta}</small> : null}
      </span>
      <span className="tracing-row-time">{time}</span>
    </button>
  );
}
