import { MarkdownHooks } from "react-markdown";
import remarkGfm from "remark-gfm";
import rehypePrettyCode from "rehype-pretty-code";
import { Bot, ChevronDown, ChevronUp, LoaderCircle, Sparkles, User, Wrench } from "lucide-react";
import { useMemo, useState, type ComponentProps, type ReactNode } from "react";

import type { ChatRenderBlock } from "@/shared/types/domain";
import { cn } from "@/shared/lib/cn";
import { Badge } from "@/shared/ui/Badge";
import { Button } from "@/shared/ui/Button";
import { Panel } from "@/shared/ui/Panel";

const markdownCodeTheme = {
  dark: "github-dark",
  light: "github-light",
} as const;

const markdownCodeOptions = {
  theme: markdownCodeTheme,
  keepBackground: false,
  defaultLang: "text",
} as const;

function ToolBlock({ block }: { block: Extract<ChatRenderBlock, { kind: "tool" }> }) {
  const [open, setOpen] = useState(block.status === "waiting_approval");

  return (
    <div className="w-full md:ml-11 md:max-w-[39rem]">
      <Panel className="overflow-hidden rounded-[1.25rem] border-border/65 bg-card/76 shadow-[0_10px_24px_rgba(95,84,64,0.035)]">
        <button
          className="flex w-full items-center justify-between gap-3 px-4 py-3 text-left"
          onClick={() => setOpen((value) => !value)}
          type="button"
        >
          <div className="flex items-center gap-3">
            <span className="rounded-[0.85rem] border border-brand/14 bg-brand/5 p-2 text-brand/85">
              <Wrench size={13} />
            </span>
            <div className="min-w-0">
              <div className="text-[13.5px] font-medium text-ink">{block.toolName}</div>
              <div className="truncate text-[11px] text-muted">{block.callId}</div>
            </div>
          </div>
          <div className="flex items-center gap-2">
            <Badge
              className="text-[10px]"
              tone={
                block.status === "done"
                  ? "success"
                  : block.status === "waiting_approval"
                    ? "warning"
                    : block.status === "error"
                      ? "danger"
                      : "default"
              }
            >
              {block.status.replaceAll("_", " ")}
            </Badge>
            {open ? <ChevronUp size={16} /> : <ChevronDown size={16} />}
          </div>
        </button>
        {open ? (
          <div className="space-y-3 border-t border-border/80 bg-background/26 px-4 py-3 text-sm text-muted">
            <section>
              <div className="mb-2 text-[10px] uppercase tracking-[0.14em] text-muted">Arguments</div>
              <pre className="overflow-x-auto rounded-[0.95rem] border border-border/75 bg-surface/88 p-3 font-mono text-[12px] leading-5 text-ink">
                {JSON.stringify(block.arguments, null, 2)}
              </pre>
            </section>
            {block.prompt ? (
              <section>
                <div className="mb-2 text-[10px] uppercase tracking-[0.14em] text-muted">Approval Prompt</div>
                <p className="rounded-[0.95rem] border border-border/70 bg-background/45 p-3 text-[13px] leading-6 text-ink">{block.prompt}</p>
              </section>
            ) : null}
            {block.progress.length > 0 ? (
              <section>
                <div className="mb-2 text-[10px] uppercase tracking-[0.14em] text-muted">Progress</div>
                <div className="space-y-1.5 rounded-[0.95rem] border border-border/75 bg-surface/88 p-3 font-mono text-[12px] leading-5">
                  {block.progress.map((entry, index) => (
                    <div key={`${block.callId}-${index}`}>
                      <span className="mr-2 text-brand">{entry.kind}</span>
                      <span>{entry.message}</span>
                    </div>
                  ))}
                </div>
              </section>
            ) : null}
            {block.output ? (
              <section>
                <div className="mb-2 text-[10px] uppercase tracking-[0.14em] text-muted">Output</div>
                <pre className="overflow-x-auto rounded-[0.95rem] border border-border/75 bg-surface/88 p-3 font-mono text-[12px] leading-5 text-ink">
                  {block.output}
                </pre>
              </section>
            ) : null}
            {block.timingMs ? <div className="text-[11px] text-muted">Duration {block.timingMs}ms</div> : null}
          </div>
        ) : null}
      </Panel>
    </div>
  );
}

function TextBlock({ block }: { block: Exclude<ChatRenderBlock, { kind: "tool" }> }) {
  if (block.kind === "status") {
    return <StatusBlock block={block} />;
  }

  const isUser = block.kind === "user";
  const isThinking = block.kind === "thinking";
  const isSystem = block.kind === "system";
  const icon = isUser ? <User size={16} /> : isThinking ? <Sparkles size={16} /> : isSystem ? <Wrench size={16} /> : <Bot size={16} />;
  const markdownComponents = useMemo(
    () => ({
      table(props: ComponentProps<"table">) {
        return (
          <div className="my-4 overflow-x-auto">
            <table {...props} />
          </div>
        );
      },
    }),
    [],
  );

  return (
    <div className={cn("flex items-start gap-3.5 md:gap-4", isUser ? "justify-end" : "justify-start")}>
      {!isUser ? (
        <div
          className={cn(
            "mt-1.5 flex size-7.5 shrink-0 items-center justify-center rounded-[0.95rem] border text-brand md:size-8",
            isThinking
              ? "border-brand/18 bg-brand/6"
              : isSystem
                ? "border-border/80 bg-background/45"
                : "border-border bg-surface/90",
          )}
        >
          {icon}
        </div>
      ) : null}
      <div
        className={cn(
          "min-w-0 rounded-[1.25rem] px-4 py-3",
          isUser
            ? "max-w-[min(100%,39rem)] border border-brand/18 bg-brandDim/95 text-ink shadow-[0_10px_24px_rgba(194,109,39,0.08)]"
            : isThinking
              ? "flex-1 border border-border/70 bg-background/34 text-muted"
              : isSystem
                ? "flex-1 border border-border/65 bg-background/40 text-muted"
                : "flex-1 border border-border/70 bg-card/88 px-5 py-4 text-ink shadow-[0_12px_28px_rgba(95,84,64,0.04)] md:px-6 md:py-[1.15rem]",
        )}
      >
        <div
          className={cn(
            "markdown-body prose prose-sm max-w-none",
            "prose-headings:font-display prose-headings:tracking-[-0.035em]",
            "prose-p:my-3 prose-p:leading-[1.9] prose-ul:my-4 prose-ol:my-4 prose-li:my-1.5 prose-li:leading-[1.85]",
            "prose-a:no-underline hover:prose-a:opacity-80",
            isThinking && "prose-p:text-muted prose-headings:text-ink",
            isSystem && "prose-p:my-0 prose-p:text-muted",
          )}
        >
          <MarkdownHooks
            fallback={<div className="text-sm text-muted">Rendering message…</div>}
            components={markdownComponents}
            rehypePlugins={[[rehypePrettyCode, markdownCodeOptions]]}
            remarkPlugins={[remarkGfm]}
          >
            {block.text}
          </MarkdownHooks>
        </div>
      </div>
      {isUser ? (
        <div className="mt-1.5 flex size-7.5 shrink-0 items-center justify-center rounded-[0.95rem] border border-brand/15 bg-brandDim text-brand md:size-8">
          {icon}
        </div>
      ) : null}
    </div>
  );
}

function StatusBlock({ block }: { block: Extract<ChatRenderBlock, { kind: "status" }> }) {
  const icon = <LoaderCircle className="animate-spin" size={16} />;
  const iconClassName =
    block.variant === "thinking"
      ? "mt-1.5 flex size-7.5 shrink-0 items-center justify-center rounded-[0.95rem] border border-brand/18 bg-brand/6 text-brand md:size-8"
      : "mt-1.5 flex size-7.5 shrink-0 items-center justify-center rounded-[0.95rem] border border-brand/16 bg-background/78 text-brand md:size-8";

  return (
    <div className="flex items-start gap-3.5 md:gap-4">
      <div className={iconClassName}>
        {icon}
      </div>
      <div className="flex-1 rounded-[1.25rem] border border-brand/16 bg-card/92 px-5 py-4 text-ink shadow-[0_14px_34px_rgba(95,84,64,0.055)]">
        <div className="text-[11px] uppercase tracking-[0.16em] text-brand/80">Assistant</div>
        <div className="mt-2 text-sm font-medium text-ink">{block.title}</div>
        <p className="mt-1.5 text-sm leading-6 text-muted">{block.detail}</p>
      </div>
    </div>
  );
}

interface MessageListProps {
  blocks: ChatRenderBlock[];
  emptyTitle: string;
  emptyDetail: string;
  actions?: ReactNode;
}

export function MessageList({ blocks, emptyTitle, emptyDetail, actions }: MessageListProps) {
  if (blocks.length === 0) {
    return (
      <div className="flex min-h-[15rem] flex-col items-center justify-center gap-4 px-8 py-8 text-center">
        <Badge className="text-brand" tone="success">
          New thread
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
      {blocks.map((block) =>
        block.kind === "tool" ? <ToolBlock key={block.id} block={block} /> : <TextBlock key={block.id} block={block} />,
      )}
    </div>
  );
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
