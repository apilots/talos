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
    <section aria-label={title} className="space-y-2">
      <h3 className="text-xs font-semibold text-ink">{title}</h3>
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
    description,
    duration,
    exitLabel,
    open,
    onToggle,
    running,
    statusLabel,
    title,
    tone,
  }: {
    compact?: boolean;
    description?: string;
    duration?: ReactNode;
    exitLabel?: string;
    open: boolean;
    onToggle: () => void;
    running: boolean;
    statusLabel?: string;
    title: string;
    tone: "default" | "success" | "warning" | "danger";
  }) {
    return (
      <button
        aria-expanded={open}
        className={cn(
          "flex w-full items-center justify-between gap-3 text-left",
          compact
            ? "group/tool rounded-[0.7rem] px-1 py-1.5 transition-colors hover:bg-hover focus:outline-none focus-visible:ring-2 focus-visible:ring-brand/20"
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
          <div
            className="min-w-0 flex-1 truncate text-[13.5px] text-muted"
            title={description ? `${title} ${description}` : title}
          >
            <span className={statusLabel || description ? "font-medium text-ink" : undefined}>
              {title}
            </span>
            {description ? <>{" "}<span>{description}</span></> : null}
          </div>
        </div>
        <div className={cn("flex shrink-0 items-center", compact ? "gap-2.5" : "gap-2")}>
          {exitLabel ? <span className="whitespace-nowrap text-[11px] tabular-nums text-muted">{exitLabel}</span> : null}
          {duration ? <span className="whitespace-nowrap text-[11px] tabular-nums text-muted" data-tool-duration="true">{duration}</span> : null}
          {statusLabel && (compact ? (
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
              ) : running ? (
                <LoaderCircle aria-hidden="true" className="motion-safe:animate-spin" size={13} />
              ) : (
                <CircleDashed size={13} />
              )}
              {statusLabel}
            </span>
          ) : (
            <Badge className="whitespace-nowrap text-[10px]" tone={tone}>
              {statusLabel}
            </Badge>
          ))}
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
  Progress({ messages, title }: { messages: string[]; title: string }) {
    return (
      <Section title={title}>
        <ol className="space-y-1 text-xs leading-5 [overflow-wrap:anywhere]" role="list">
          {messages.map((message, index) => (
            <li className="flex items-start gap-2 whitespace-pre-wrap" key={index}>
              <span aria-hidden="true" className="mt-[3px] flex size-3.5 shrink-0 items-center justify-center">
                <span className="size-1 rounded-full bg-muted/60" />
              </span>
              <span className="min-w-0">{message}</span>
            </li>
          ))}
        </ol>
      </Section>
    );
  },
  Details({ children, compact = false, onOpen, title }: {
    children: ReactNode;
    compact?: boolean;
    onOpen?: () => void;
    title: string;
  }) {
    return (
      <details
        className={cn(
          "group/details",
          compact ? "border-t border-border/65" : "rounded-lg border border-border/65 bg-background/40",
        )}
      >
        <summary
          onClick={(event) => {
            // Native pointer and keyboard activation both click before toggling.
            if (event.currentTarget.closest("details")?.open === false) onOpen?.();
          }}
          className={cn(
            "flex list-none items-center gap-1.5 rounded-md cursor-pointer text-[11px] font-medium uppercase tracking-[0.12em] text-muted marker:hidden [&::-webkit-details-marker]:hidden",
            "transition-colors hover:text-ink focus-visible:outline focus-visible:outline-2 focus-visible:outline-brand",
            compact ? "px-1 py-2 hover:bg-hover" : "px-3 py-2",
          )}
        >
          {title}
          <ChevronDown aria-hidden="true" className="shrink-0 group-open/details:rotate-180" size={13} />
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
