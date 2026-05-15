import type { ReactNode } from "react";
import { ChevronDown, ChevronUp, Wrench } from "lucide-react";

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
    return (
      <div className={cn("w-full", !compact && "md:ml-11 md:max-w-[39rem]")}>
        <Panel
          className={cn(
            "overflow-hidden border-border/65 bg-card/76",
            compact
              ? "rounded-[0.85rem] shadow-none"
              : "rounded-[1.25rem] shadow-[0_10px_24px_rgba(95,84,64,0.035)]",
          )}
        >
          {children}
        </Panel>
      </div>
    );
  },
  Header({
    callId,
    open,
    onToggle,
    statusLabel,
    title,
    tone,
  }: {
    callId: string;
    open: boolean;
    onToggle: () => void;
    statusLabel: string;
    title: string;
    tone: "default" | "success" | "warning" | "danger";
  }) {
    return (
      <button
        className="flex w-full items-center justify-between gap-3 px-4 py-3 text-left"
        onClick={onToggle}
        type="button"
      >
        <div className="flex items-center gap-3">
          <span className="rounded-[0.85rem] border border-brand/14 bg-brand/5 p-2 text-brand/85">
            <Wrench size={13} />
          </span>
          <div className="min-w-0">
            <div className="text-[13.5px] font-medium text-ink">{title}</div>
            <div className="truncate text-[11px] text-muted">{callId}</div>
          </div>
        </div>
        <div className="flex items-center gap-2">
          <Badge className="text-[10px]" tone={tone}>
            {statusLabel}
          </Badge>
          {open ? <ChevronUp size={16} /> : <ChevronDown size={16} />}
        </div>
      </button>
    );
  },
  Body({ children }: { children: ReactNode }) {
    return (
      <div className="space-y-3 border-t border-border/80 bg-background/26 px-4 py-3 text-sm text-muted">
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
        <pre className="overflow-x-auto rounded-[0.95rem] border border-border/75 bg-surface/88 p-3 font-mono text-[12px] leading-5 text-ink">
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
        <div className="space-y-1.5 rounded-[0.95rem] border border-border/75 bg-surface/88 p-3 font-mono text-[12px] leading-5">
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
  Output({
    content,
    title,
  }: {
    content: string;
    title: string;
  }) {
    return (
      <Section title={title}>
        <pre className="overflow-x-auto rounded-[0.95rem] border border-border/75 bg-surface/88 p-3 font-mono text-[12px] leading-5 text-ink">
          {content}
        </pre>
      </Section>
    );
  },
  ApprovalActions({
    children,
    prompt,
    title,
  }: {
    children?: ReactNode;
    prompt: string;
    title: string;
  }) {
    return (
      <Section title={title}>
        <div
          className={cn(
            "rounded-[0.95rem] border border-border/70 bg-background/45 p-3 text-[13px] leading-6 text-ink",
            children ? "space-y-3" : "",
          )}
        >
          <p>{prompt}</p>
          {children}
        </div>
      </Section>
    );
  },
};
