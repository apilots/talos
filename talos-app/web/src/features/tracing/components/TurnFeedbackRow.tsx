import { useTranslation } from "react-i18next";
import type { AppProtocol as API } from "@talos/sdk-core";
import { blockLabel, blockText, type TraceSelection } from "../model/selectors";
import { TraceRow } from "./TraceRow";
export function TurnFeedbackRow({
  block,
  turnId,
  onSelect,
  selected = false,
}: {
  block: API.AgentMessageBlock;
  turnId: string;
  onSelect: (selection: TraceSelection, button: HTMLButtonElement) => void;
  selected?: boolean;
}) {
  const { t } = useTranslation("tracing"),
    kind = block.content.type;
  const tag =
    kind === "reasoning"
      ? "THINKING"
      : kind === "assistantText"
        ? block.content.data.phase === "finalAnswer"
          ? "ASSISTANT"
          : block.content.data.phase === "pending"
            ? "PENDING"
            : "PROGRESS"
        : kind === "executionSummary"
          ? "PROGRESS"
          : kind === "userContent"
            ? "USER"
            : kind === "toolCall" || kind === "toolResult"
              ? "TOOL"
              : "RECORD";
  return (
    <TraceRow
      kind={kind}
      tag={tag}
      selected={selected}
      title={t(blockLabel(block))}
      preview={blockText(block)}
      meta={t(`states.${block.status}`)}
      status={block.status}
      onClick={(e) =>
        onSelect(
          block.content.type === "toolCall" ||
            block.content.type === "toolResult"
            ? { kind: "tool", turnId, callId: block.content.data.callId }
            : { kind: "fact", turnId, block },
          e.currentTarget,
        )
      }
    />
  );
}
