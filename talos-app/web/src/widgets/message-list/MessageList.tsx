import type { ReactNode } from "react";
import type { PendingUserAction } from "@talos/sdk-browser";
import { useTranslation } from "react-i18next";

import type { ChatRenderBlock } from "@/shared/types/ui";
import { Badge } from "@/shared/ui/Badge";
import { Button } from "@/shared/ui/Button";
import { CopyableBlock } from "./CopyableBlock";
import { MessageTimelineBlock } from "./MessageTimelineBlock";
import { StatusMessageBlock } from "./StatusMessageBlock";
import { TextMessageBlock } from "./TextMessageBlock";
import { ToolCallBlock } from "./ToolCallBlock";

interface MessageListProps {
  blocks: ChatRenderBlock[];
  emptyTitle: string;
  emptyDetail: string;
  actions?: ReactNode;
  pendingUserAction?: PendingUserAction;
  renderUserAction?: (action: PendingUserAction) => ReactNode;
}

export function MessageList({
  blocks,
  emptyTitle,
  emptyDetail,
  actions,
  pendingUserAction,
  renderUserAction,
}: MessageListProps) {
  const { t } = useTranslation("chat");
  const copyBlockLabel = t("copyBlock");
  const copiedBlockLabel = t("copiedBlock");
  const helpfulLabel = t("helpful");
  const notHelpfulLabel = t("notHelpful");

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
        if (block.kind === "timeline") {
          return (
            <MessageTimelineBlock
              block={block}
              copiedLabel={copiedBlockLabel}
              copyLabel={copyBlockLabel}
              helpfulLabel={helpfulLabel}
              key={block.id}
              notHelpfulLabel={notHelpfulLabel}
              pendingUserAction={pendingUserAction}
              renderUserAction={renderUserAction}
            />
          );
        }
        if (block.kind === "user") {
          return (
            <TextMessageBlock
              fileIds={block.fileIds}
              key={block.id}
              kind={block.kind}
              messageId={block.id}
              text={block.text}
            />
          );
        }
        const copyText = blockToMarkdown(block);
        if (block.kind === "tool") {
          return (
            <CopyableBlock copyLabel={copyBlockLabel} copyText={copyText} copiedLabel={copiedBlockLabel} key={block.id}>
              <ToolCallBlock
                argumentsText={JSON.stringify(block.arguments, null, 2)}
                callId={block.callId}
                contentItems={block.contentItems}
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
          <CopyableBlock
            copiedLabel={copiedBlockLabel}
            copyLabel={copyBlockLabel}
            copyText={copyText}
            helpfulLabel={helpfulLabel}
            key={block.id}
            notHelpfulLabel={notHelpfulLabel}
            responseActionInset={block.kind === "assistant"}
            variant={block.kind === "assistant" ? "response" : "hover"}
          >
            <TextMessageBlock
              kind={block.kind}
              text={block.text}
            />
          </CopyableBlock>
        );
      })}
    </div>
  );
}

function blockToMarkdown(block: Exclude<ChatRenderBlock, { kind: "timeline" | "user" }>) {
  switch (block.kind) {
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
    sections.push("#### Dynamic Tool Prompt", block.prompt);
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
