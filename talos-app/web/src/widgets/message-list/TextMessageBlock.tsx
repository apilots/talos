import { Sparkles, Wrench } from "lucide-react";
import { lazy, Suspense } from "react";
import { useTranslation } from "react-i18next";

import { MessageAttachments } from "./MessageAttachments";

import { cn } from "@/shared/lib/cn";
const MarkdownRenderer = lazy(() =>
  import("./MarkdownRenderer").then((module) => ({ default: module.MarkdownRenderer })),
);

export function TextMessageBlock({
  fileIds = [],
  kind,
  messageId,
  text,
}: {
  fileIds?: string[];
  kind: "user" | "assistant" | "thinking" | "system";
  messageId?: string;
  text: string;
}) {
  const { t } = useTranslation("chat");
  const isUser = kind === "user";
  const isThinking = kind === "thinking";
  const isSystem = kind === "system";
  const icon = isThinking ? <Sparkles size={16} /> : <Wrench size={16} />;
  return (
    <div
      className={cn("flex items-start gap-3.5 md:gap-4", isUser ? "justify-end" : "justify-start")}
      data-chat-user-block-id={isUser ? messageId : undefined}
    >
      {isThinking || isSystem ? (
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
          "min-w-0",
          isUser
            ? "max-w-[min(100%,39rem)] rounded-[1.25rem] border border-border/70 bg-surface px-4 py-3 text-ink"
            : isThinking
              ? "flex-1 rounded-[1.25rem] border border-border/70 bg-background/34 px-4 py-3 text-muted"
              : isSystem
                ? "flex-1 rounded-[1.25rem] border border-border/65 bg-background/40 px-4 py-3 text-muted"
                : "flex-1 px-1 py-2 text-ink",
        )}
        data-response-surface={!isUser && !isThinking && !isSystem ? "plain" : undefined}
      >
        <div>
          {isUser ? (
            <div className="space-y-2.5">
              {fileIds.length > 0 ? <MessageAttachments fileIds={fileIds} /> : null}
              {text ? (
                <div className="whitespace-pre-wrap break-words text-sm leading-[1.7]">{text}</div>
              ) : null}
            </div>
          ) : (
            <Suspense fallback={<div className="text-sm text-muted">{t("renderingMessage")}</div>}>
              <MarkdownRenderer
                className={cn(
                  "markdown-body prose prose-sm max-w-none",
                  "prose-headings:font-display prose-headings:tracking-[-0.035em]",
                  "prose-p:my-3 prose-p:leading-[1.9] prose-ul:my-4 prose-ol:my-4 prose-li:my-1.5 prose-li:leading-[1.85]",
                  "prose-a:no-underline hover:prose-a:opacity-80",
                  isThinking && "prose-p:text-muted prose-headings:text-ink",
                  isSystem && "prose-p:my-0 prose-p:text-muted",
                )}
                text={text}
              />
            </Suspense>
          )}
        </div>
      </div>
    </div>
  );
}
