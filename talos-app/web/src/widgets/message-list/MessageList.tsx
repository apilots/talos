import type { ReactNode } from "react";
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
        if (block.kind === "tool") {
          return (
            <ToolCallBlock
              argumentsText={JSON.stringify(block.arguments, null, 2)}
              callId={block.callId}
              key={block.id}
              output={block.output}
              progress={block.progress.map((entry) => ({ kind: entry.kind, message: entry.message }))}
              prompt={block.prompt}
              status={block.status}
              timingMs={block.timingMs}
              toolName={block.toolName}
            />
          );
        }

        if (block.kind === "status") {
          return (
            <StatusMessageBlock
              detail={block.detail}
              key={block.id}
              title={block.title}
              variant={block.variant}
            />
          );
        }

        return <TextMessageBlock key={block.id} kind={block.kind} text={block.text} />;
      })}
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
