import type { ReactNode } from "react";
import { Check, Clock3, Copy, LoaderCircle, Sparkles, Wrench } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import type { ChatRenderBlock } from "@/shared/types/ui";
import { Badge } from "@/shared/ui/Badge";
import { Button } from "@/shared/ui/Button";
import { ToolCallBlock } from "./ToolCallBlock";
import { TextMessageBlock } from "./TextMessageBlock";
import { StatusMessageBlock } from "./StatusMessageBlock";
import { formatDuration } from "./formatDuration";

interface MessageListProps {
  blocks: ChatRenderBlock[];
  emptyTitle: string;
  emptyDetail: string;
  actions?: ReactNode;
}

export function MessageList({ blocks, emptyTitle, emptyDetail, actions }: MessageListProps) {
  const { t } = useTranslation("chat");
  const copyBlockLabel = t("copyBlock");
  const copiedBlockLabel = t("copiedBlock");

  if (blocks.length === 0) {
    return (
      <div className="flex min-h-[15rem] flex-col items-center justify-center gap-4 px-8 py-8 text-center">
        <Badge className="text-brand" tone="success">
          {t("newThread")}
        </Badge>
        <div className="max-w-xl space-y-3">
          <h1 className="font-display text-[30px] font-semibold tracking-[-0.05em]">{emptyTitle}</h1>
          <p className="text-sm leading-6 text-muted">{emptyDetail}</p>
        </div>
        {actions ? <div className="flex flex-wrap items-center justify-center gap-3">{actions}</div> : null}
      </div>
    );
  }

  return (
    <div className="space-y-5 md:space-y-6">
      {blocks.map((block) => {
        const copyText = blockToMarkdown(block);
        if (block.kind === "tool") {
          return (
            <CopyableBlock copyLabel={copyBlockLabel} copyText={copyText} copiedLabel={copiedBlockLabel} key={block.id}>
              <ToolCallBlock
                argumentsText={JSON.stringify(block.arguments, null, 2)}
                callId={block.callId}
                output={block.output}
                progress={block.progress.map((entry) => ({ kind: entry.kind, message: entry.message }))}
                prompt={block.prompt}
                status={block.status}
                timingMs={block.timingMs}
                toolName={block.toolName}
              />
            </CopyableBlock>
          );
        }

        if (block.kind === "status") {
          return (
            <CopyableBlock copyLabel={copyBlockLabel} copyText={copyText} copiedLabel={copiedBlockLabel} key={block.id}>
              <StatusMessageBlock
                detail={block.detail}
                title={block.title}
                variant={block.variant}
              />
            </CopyableBlock>
          );
        }

        if (block.kind === "timeline") {
          return (
            <CopyableBlock copyLabel={copyBlockLabel} copyText={copyText} copiedLabel={copiedBlockLabel} key={block.id}>
              <TimelineBlock items={block.items} live={Boolean(block.live)} />
            </CopyableBlock>
          );
        }

        return (
          <CopyableBlock copyLabel={copyBlockLabel} copyText={copyText} copiedLabel={copiedBlockLabel} key={block.id}>
            <TextMessageBlock kind={block.kind} text={block.text} />
          </CopyableBlock>
        );
      })}
    </div>
  );
}

function CopyableBlock({
  children,
  copiedLabel,
  copyLabel,
  copyText,
}: {
  children: ReactNode;
  copiedLabel: string;
  copyLabel: string;
  copyText: string;
}) {
  const [copied, setCopied] = useState(false);
  const resetTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    return () => {
      if (resetTimerRef.current) {
        clearTimeout(resetTimerRef.current);
      }
    };
  }, []);

  const copyBlock = async () => {
    if (!copyText) {
      return;
    }
    await writeClipboardText(copyText);
    setCopied(true);
    if (resetTimerRef.current) {
      clearTimeout(resetTimerRef.current);
    }
    resetTimerRef.current = setTimeout(() => setCopied(false), 1200);
  };

  return (
    <div className="group relative">
      {children}
      <button
        aria-label={copied ? copiedLabel : copyLabel}
        className="absolute right-2 top-2 z-10 flex size-8 items-center justify-center rounded-[0.7rem] border border-border/70 bg-card/95 text-muted opacity-0 shadow-[0_10px_24px_rgba(95,84,64,0.08)] transition hover:border-brand/35 hover:text-brand focus:opacity-100 focus:outline-none focus:ring-2 focus:ring-brand/30 group-hover:opacity-100"
        onClick={() => void copyBlock()}
        title={copied ? copiedLabel : copyLabel}
        type="button"
      >
        {copied ? <Check size={15} /> : <Copy size={15} />}
      </button>
    </div>
  );
}

async function writeClipboardText(text: string) {
  if (navigator.clipboard?.writeText) {
    await navigator.clipboard.writeText(text);
    return;
  }

  const textarea = document.createElement("textarea");
  textarea.value = text;
  textarea.setAttribute("readonly", "true");
  textarea.style.position = "fixed";
  textarea.style.left = "-9999px";
  document.body.appendChild(textarea);
  textarea.select();
  document.execCommand("copy");
  document.body.removeChild(textarea);
}

function blockToMarkdown(block: ChatRenderBlock) {
  switch (block.kind) {
    case "user":
    case "assistant":
    case "thinking":
    case "system":
      return block.text;
    case "status":
      return [`**${block.title}**`, block.detail].filter(Boolean).join("\n\n");
    case "tool":
      return toolBlockToMarkdown(block);
    case "timeline":
      return timelineBlockToMarkdown(block);
  }
}

function TimelineBlock({
  items,
  live,
}: {
  items: Extract<ChatRenderBlock, { kind: "timeline" }>["items"];
  live: boolean;
}) {
  const hasRunningItem = items.some((item) => item.status === "running" || item.status === "waiting_approval");
  const hasActiveTimeline = live || hasRunningItem;
  const isAwaitingTimelineDelta =
    items.some((item) => item.status === "running") &&
    !items.some((item) => item.phase === "final_answer");
  const [nowMs, setNowMs] = useState(() => Date.now());
  const totalDuration = timelineDuration(items, nowMs);
  const activitySignature = timelineActivitySignature(items);
  const [open, setOpen] = useState(hasActiveTimeline);
  const wasActiveRef = useRef(hasActiveTimeline);
  const activitySignatureRef = useRef(activitySignature);

  useEffect(() => {
    if (!hasRunningItem) {
      return;
    }
    const timer = window.setInterval(() => setNowMs(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, [hasRunningItem]);

  useEffect(() => {
    const changed = activitySignatureRef.current !== activitySignature;
    if (hasActiveTimeline && (!wasActiveRef.current || changed)) {
      setOpen(true);
    }
    if (!hasActiveTimeline && wasActiveRef.current) {
      setOpen(false);
    }
    wasActiveRef.current = hasActiveTimeline;
    activitySignatureRef.current = activitySignature;
  }, [activitySignature, hasActiveTimeline]);

  return (
    <div className="flex items-start gap-3.5 md:gap-4">
      <div className="mt-1.5 flex size-7.5 shrink-0 items-center justify-center rounded-[0.95rem] border border-brand/18 bg-brand/6 text-brand md:size-8">
        <Sparkles size={16} />
      </div>
      <details
        className="group/timeline flex-1 rounded-[1.1rem] border border-border/70 bg-background/38 text-ink shadow-[0_10px_26px_rgba(95,84,64,0.035)]"
        open={open}
      >
        <summary
          className="flex cursor-pointer list-none items-center justify-between gap-3 px-4 py-3 text-sm font-medium marker:hidden"
          onClick={(event) => {
            event.preventDefault();
            setOpen((current) => !current);
          }}
        >
          <span className="flex min-w-0 items-center gap-2">
            <span>Thinking / Acting</span>
            <span className="rounded-full border border-border/70 px-2 py-0.5 text-[11px] font-normal text-muted">
              {items.length}
            </span>
          </span>
          {totalDuration !== null ? (
            <span className="flex shrink-0 items-center gap-1.5 text-[11px] font-normal text-muted">
              <Clock3 size={13} />
              {formatDuration(totalDuration)}
            </span>
          ) : null}
        </summary>
        <ol className="space-y-3 border-t border-border/60 px-3 py-3">
          {items.map((item, index) => {
            const statusLabel = visibleTimelineItemStatus(item);
            return (
              <li className="relative grid grid-cols-[1.55rem_minmax(0,1fr)] gap-2.5" key={item.itemId}>
                {index < items.length - 1 || isAwaitingTimelineDelta ? (
                  <span className="absolute left-[0.72rem] top-8 h-[calc(100%-1.25rem)] w-px bg-border/70" />
                ) : null}
                <span className="relative z-[1] mt-2 flex size-6 items-center justify-center rounded-full border border-brand/18 bg-card text-brand shadow-[0_3px_10px_rgba(95,84,64,0.05)]">
                  <TimelineItemIcon kind={item.kind} />
                </span>
                <article className="min-w-0 rounded-[0.9rem] border border-border/65 bg-card/72">
                  <header className="flex items-center justify-between gap-3 px-3 py-2.5">
                    <span className="flex min-w-0 items-center gap-2">
                      <span className="truncate text-sm font-medium">{timelineItemTitle(item)}</span>
                    </span>
                    {statusLabel ? <span className="shrink-0 text-[11px] text-muted">{statusLabel}</span> : null}
                  </header>
                  <div className="border-t border-border/55 px-3 py-3">
                    {item.kind === "tool_call" ? (
                      <ToolCallBlock
                        argumentsText={formatArguments(item.arguments)}
                        callId={item.callId}
                        output={item.output}
                        progress={item.progress}
                        prompt={item.prompt}
                        status={timelineToolStatus(item)}
                        timingMs={timelineItemDuration(item)}
                        toolName={item.toolName}
                        variant="timeline"
                      />
                    ) : (
                      <TimelineThoughtLines text={item.text} />
                    )}
                  </div>
                </article>
              </li>
            );
          })}
          {isAwaitingTimelineDelta ? (
            <li className="relative grid grid-cols-[1.55rem_minmax(0,1fr)] gap-2.5" key="timeline-waiting-indicator">
              <span
                aria-label="Waiting for timeline update"
                className="relative z-[1] mt-1 flex size-6 items-center justify-center rounded-full border border-brand/18 bg-card text-brand shadow-[0_3px_10px_rgba(95,84,64,0.05)]"
              >
                <LoaderCircle className="animate-spin" size={13} />
              </span>
              <span className="min-h-7" />
            </li>
          ) : null}
        </ol>
      </details>
    </div>
  );
}

function TimelineItemIcon({
  kind,
}: {
  kind: Extract<ChatRenderBlock, { kind: "timeline" }>["items"][number]["kind"];
}) {
  if (kind === "tool_call") {
    return <Wrench className="shrink-0 text-brand" size={13} />;
  }
  return <Sparkles className="shrink-0 text-brand" size={13} />;
}

function TimelineThoughtLines({ text }: { text: string }) {
  const segments = timelineTextSegments(text);
  if (segments.length === 0) {
    return <div className="text-sm leading-6 text-muted">…</div>;
  }
  return (
    <div className="space-y-2">
      {segments.map((segment, index) => (
        <div className="flex gap-2.5 text-sm leading-6 text-muted" key={`${index}-${segment.slice(0, 24)}`}>
          <span className="mt-[0.62rem] size-1.5 shrink-0 rounded-full bg-brand/45" />
          <p className="min-w-0 whitespace-pre-wrap break-words">{segment}</p>
        </div>
      ))}
    </div>
  );
}

function timelineTextSegments(text: string) {
  const trimmed = text.trim();
  if (!trimmed) {
    return [];
  }
  return trimmed
    .split(/\n{2,}/)
    .flatMap((paragraph) => {
      const value = paragraph.trim();
      if (!value) {
        return [];
      }
      if (/[`#]|(?:^|\n)\s*(?:[-*]|\d+\.)\s+/.test(value)) {
        return [value];
      }
      return value.match(/[^。！？.!?\n]+[。！？.!?]?/g)?.map((segment) => segment.trim()).filter(Boolean) ?? [value];
    });
}

function timelineItemTitle(item: Extract<ChatRenderBlock, { kind: "timeline" }>["items"][number]) {
  if (item.kind === "tool_call") {
    return item.toolName;
  }
  if (item.phase === "thinking" || item.phase === "commentary") {
    return "Thinking";
  }
  if (item.phase === "system") {
    return item.title || "System";
  }
  return item.title || item.kind.replaceAll("_", " ");
}

function timelineItemStatus(item: Extract<ChatRenderBlock, { kind: "timeline" }>["items"][number]) {
  const duration = timelineItemDuration(item);
  return duration === null ? item.status.replaceAll("_", " ") : `${item.status.replaceAll("_", " ")} · ${formatDuration(duration)}`;
}

function visibleTimelineItemStatus(item: Extract<ChatRenderBlock, { kind: "timeline" }>["items"][number]) {
  if (item.kind === "tool_call") {
    const duration = timelineItemDuration(item);
    return duration === null ? undefined : formatDuration(duration);
  }
  if (item.status === "failed" || item.status === "cancelled") {
    return timelineItemStatus(item);
  }
  return undefined;
}

function timelineItemDuration(item: Extract<ChatRenderBlock, { kind: "timeline" }>["items"][number]) {
  if (!item.completedAtMs) {
    return null;
  }
  return Math.max(0, item.completedAtMs - item.startedAtMs);
}

function timelineActivitySignature(items: Extract<ChatRenderBlock, { kind: "timeline" }>["items"]) {
  return items
    .map((item) => {
      if (item.kind === "tool_call") {
        const argumentsLength = typeof item.arguments === "string" ? item.arguments.length : (JSON.stringify(item.arguments)?.length ?? 0);
        return [
          item.itemId,
          item.status,
          item.progress.length,
          item.output?.length ?? 0,
          argumentsLength,
          item.completedAtMs ?? "",
        ].join(":");
      }
      return [
        item.itemId,
        item.status,
        item.text.length,
        item.completedAtMs ?? "",
      ].join(":");
    })
    .join("|");
}

function timelineDuration(items: Extract<ChatRenderBlock, { kind: "timeline" }>["items"], nowMs: number) {
  const started = items.map((item) => item.startedAtMs).filter((value) => value > 0);
  const completed = items
    .map((item) => item.completedAtMs ?? (item.status === "running" ? nowMs : null))
    .filter((value): value is number => value !== null && value > 0);
  if (started.length === 0 || completed.length === 0) {
    return null;
  }
  return Math.max(0, Math.max(...completed) - Math.min(...started));
}

function timelineToolStatus(item: Extract<ChatRenderBlock, { kind: "timeline" }>["items"][number]) {
  if (item.kind !== "tool_call") {
    return "queued";
  }
  if (item.status === "completed") {
    return item.isError ? "error" : "done";
  }
  if (item.status === "failed") {
    return "error";
  }
  if (item.status === "waiting_approval") {
    return "waiting_approval";
  }
  return "running";
}

function formatArguments(value: unknown) {
  if (typeof value === "string") {
    return value;
  }
  return JSON.stringify(value, null, 2);
}

function toolBlockToMarkdown(block: Extract<ChatRenderBlock, { kind: "tool" }>) {
  const sections = [
    `### Tool: ${block.toolName}`,
    `- Call ID: \`${block.callId}\``,
    `- Status: \`${block.status}\``,
  ];

  if (block.timingMs) {
    sections.push(`- Duration: ${block.timingMs}ms`);
  }

  sections.push("#### Arguments", fencedMarkdown(JSON.stringify(block.arguments, null, 2), "json"));

  if (block.prompt) {
    sections.push("#### Approval Prompt", block.prompt);
  }

  if (block.progress.length > 0) {
    sections.push(
      "#### Progress",
      block.progress.map((entry) => `- **${entry.kind}**: ${entry.message}`).join("\n"),
    );
  }

  if (block.output) {
    sections.push("#### Output", fencedMarkdown(block.output));
  }

  return sections.join("\n\n");
}

function timelineBlockToMarkdown(block: Extract<ChatRenderBlock, { kind: "timeline" }>) {
  return block.items.map((item) => {
    const sections = [
      `### ${timelineItemTitle(item)}`,
      `- Kind: \`${item.kind}\``,
      `- Phase: \`${item.phase}\``,
      `- Status: \`${item.status}\``,
    ];
    const duration = timelineItemDuration(item);
    if (duration !== null) {
      sections.push(`- Duration: ${duration}ms`);
    }
    if (item.kind === "tool_call") {
      sections.push("#### Arguments", fencedMarkdown(formatArguments(item.arguments), "json"));
      if (item.progress.length > 0) {
        sections.push(
          "#### Progress",
          item.progress.map((entry) => `- **${entry.kind}**: ${entry.message}`).join("\n"),
        );
      }
      if (item.output) {
        sections.push("#### Output", fencedMarkdown(item.output));
      }
    } else if (item.text) {
      sections.push(item.text);
    }
    return sections.join("\n\n");
  }).join("\n\n");
}

function fencedMarkdown(content: string, language = "") {
  const longestFence = Math.max(3, ...Array.from(content.matchAll(/`+/g), (match) => match[0].length + 1));
  const fence = "`".repeat(longestFence);
  return `${fence}${language}\n${content}\n${fence}`;
}

export function EmptyAction({
  title,
  onClick,
}: {
  title: string;
  onClick: () => void;
}) {
  return (
    <Button onClick={onClick} variant="secondary">
      {title}
    </Button>
  );
}
