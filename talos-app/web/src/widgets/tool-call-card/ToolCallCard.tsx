import type { ReactNode } from "react";
import {
  ChevronDown,
  ChevronUp,
  CircleAlert,
  CircleCheck,
  CircleDashed,
  CircleX,
  LoaderCircle,
  Wrench,
} from "lucide-react";

import { cn } from "@/shared/lib/cn";
import { Badge } from "@/shared/ui/Badge";
import { Panel } from "@/shared/ui/Panel";

function Section({
  children,
  title,
}: {
  children: ReactNode;
  title: string;
}) {
  return (
    <section>
      <div className="mb-2 text-[10px] uppercase tracking-[0.14em] text-muted">{title}</div>
      {children}
    </section>
  );
}

export const ToolCallCard = {
  Root({ children, compact = false }: { children: ReactNode; compact?: boolean }) {
    if (compact) {
      return (
        <div
          className="w-full"
          data-tool-call-card="true"
          data-tool-call-variant="compact"
          data-tool-surface="plain"
        >
          {children}
        </div>
      );
    }
    return (
      <div
        className="w-full md:ml-11 md:max-w-[39rem]"
        data-tool-call-card="true"
        data-tool-call-variant="default"
        data-tool-surface="card"
      >
        <Panel
          className="overflow-hidden rounded-[var(--radius-panel)] border-border/65 bg-card shadow-none"
        >
          {children}
        </Panel>
      </div>
    );
  },
  Header({
    compact = false,
    durationLabel,
    open,
    onToggle,
    statusLabel,
    title,
    tone,
  }: {
    compact?: boolean;
    durationLabel?: string;
    open: boolean;
    onToggle: () => void;
    statusLabel: string;
    title: string;
    tone: "default" | "success" | "warning" | "danger";
  }) {
    return (
      <button
        aria-expanded={open}
        className={cn(
          "flex w-full items-center justify-between gap-3 text-left",
          compact
            ? "group/tool rounded-[0.7rem] px-1 py-1.5 transition-colors hover:bg-hover focus:outline-none focus:ring-2 focus:ring-brand/20"
            : "px-4 py-3",
        )}
        onClick={onToggle}
        type="button"
      >
        <div className="flex min-w-0 flex-1 items-center gap-3">
          {!compact ? (
            <span className="shrink-0 rounded-[0.85rem] border border-brand/[0.14] bg-brand/5 p-2 text-brand/85">
              <Wrench size={13} />
            </span>
          ) : null}
          <div className="min-w-0 flex-1">
            <div className="truncate text-[13.5px] font-medium text-ink" title={title}>
              {title}
            </div>
          </div>
        </div>
        <div className={cn("flex shrink-0 items-center", compact ? "gap-2.5" : "gap-2")}>
          {durationLabel ? <span className="text-[11px] text-muted">{durationLabel}</span> : null}
          {compact ? (
            <span
              className={cn(
                "flex items-center gap-1.5 whitespace-nowrap text-[10.5px] font-medium",
                tone === "success"
                  ? "text-success"
                  : tone === "warning"
                    ? "text-warning"
                    : tone === "danger"
                      ? "text-danger"
                      : "text-muted",
              )}
              data-tool-status-presentation="inline"
            >
              {tone === "success" ? (
                <CircleCheck size={13} />
              ) : tone === "warning" ? (
                <CircleAlert size={13} />
              ) : tone === "danger" ? (
                <CircleX size={13} />
              ) : statusLabel === "Running" ? (
                <LoaderCircle className="animate-spin" size={13} />
              ) : (
                <CircleDashed size={13} />
              )}
              {statusLabel}
            </span>
          ) : (
            <Badge className="whitespace-nowrap text-[10px]" tone={tone}>
              {statusLabel}
            </Badge>
          )}
          <span className={compact ? "text-muted transition-colors group-hover/tool:text-ink" : undefined}>
            {open ? <ChevronUp size={compact ? 14 : 16} /> : <ChevronDown size={compact ? 14 : 16} />}
          </span>
        </div>
      </button>
    );
  },
  Body({ children, compact = false }: { children: ReactNode; compact?: boolean }) {
    return (
      <div
        className={cn(
          "space-y-3 text-sm text-muted",
          compact ? "px-1 pb-2 pt-1.5" : "border-t border-border/80 bg-background/[0.26] px-4 py-3",
        )}
        data-tool-body-surface={compact ? "plain" : "panel"}
      >
        {children}
      </div>
    );
  },
  Input({
    content,
    title,
  }: {
    content: string;
    title: string;
  }) {
    return (
      <Section title={title}>
        <pre className="overflow-x-auto rounded-[0.95rem] border border-border/75 bg-surface/[0.88] p-3 font-mono text-[12px] leading-5 text-ink">
          {content}
        </pre>
      </Section>
    );
  },
  Progress({
    entries,
    title,
  }: {
    entries: Array<{ kind: string; message: string }>;
    title: string;
  }) {
    return (
      <Section title={title}>
        <div className="space-y-1.5 rounded-[0.95rem] border border-border/75 bg-surface/[0.88] p-3 font-mono text-[12px] leading-5">
          {entries.map((entry, index) => (
            <div key={`${entry.kind}-${index}`}>
              <span className="mr-2 text-brand">{entry.kind}</span>
              <span>{entry.message}</span>
            </div>
          ))}
        </div>
      </Section>
    );
  },
  Details({ children, compact = false, title }: { children: ReactNode; compact?: boolean; title: string }) {
    return (
      <details
        className={cn(
          compact ? "rounded-lg bg-transparent" : "rounded-lg border border-border/65 bg-background/40",
        )}
      >
        <summary
          className={cn(
            "cursor-pointer text-[11px] uppercase tracking-[0.12em] text-muted",
            compact ? "rounded-md px-1 py-2 hover:bg-hover" : "px-3 py-2",
          )}
        >
          {title}
        </summary>
        <div className={compact ? "space-y-3 px-1 pb-1 pt-2" : "space-y-3 border-t border-border/60 p-3"}>{children}</div>
      </details>
    );
  },
  MetaLine({ label, value }: { label: string; value: string }) {
    return (
      <div className="grid grid-cols-[5rem_minmax(0,1fr)] gap-2 text-[12px]">
        <span className="text-muted">{label}</span>
        <span className="truncate font-mono text-ink">{value}</span>
      </div>
    );
  },
  Prompt({
    prompt,
    title,
  }: {
    prompt: string;
    title: string;
  }) {
    return (
      <Section title={title}>
        <div
          className="rounded-[0.95rem] border border-border/70 bg-background/45 p-3 text-[13px] leading-6 text-ink"
        >
          <p>{prompt}</p>
        </div>
      </Section>
    );
  },
};
