import type { ReactNode } from "react";
import { Check, Copy } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import type { ChatRenderBlock } from "@/shared/types/ui";
import { Badge } from "@/shared/ui/Badge";
import { Button } from "@/shared/ui/Button";
import { ToolCallBlock } from "./ToolCallBlock";
import { TextMessageBlock } from "./TextMessageBlock";
import { StatusMessageBlock } from "./StatusMessageBlock";

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
  }
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
