import { useTranslation } from "react-i18next";
import type { AppProtocol as API } from "@talos/sdk-core";
import { milliseconds } from "../model/formatters";
import {
  canonicalTool,
  toolArgumentsText,
  type TraceSelection,
  type TraceTurn,
} from "../model/selectors";
import { TraceRow } from "./TraceRow";
export function ToolTraceRow({
  tool,
  turn,
  selection,
  onSelect,
}: {
  tool: API.TrajectoryTool;
  turn: TraceTurn;
  selection: TraceSelection | null;
  onSelect: (selection: TraceSelection, button: HTMLButtonElement) => void;
}) {
  const { t, i18n } = useTranslation("tracing");
  const canonical = canonicalTool(turn, tool.callId),
    intervals = tool.intervals?.filter((i) => i.kind === "execution"),
    elapsed =
      intervals?.length && intervals.every((i) => i.interval.elapsedMs != null)
        ? intervals.reduce((n, i) => n + i.interval.elapsedMs!, 0)
        : null;
  return (
    <TraceRow
      kind="toolCall"
      tag="TOOL"
      selected={selection?.kind === "tool" && selection.callId === tool.callId}
      title={canonical.call?.name ?? canonical.result?.toolName ?? tool.callId}
      preview={
        <>
          {canonical.call
            ? toolArgumentsText(canonical.call.arguments)
            : tool.callId}{" "}
          →{" "}
          {canonical.result
            ? JSON.stringify(canonical.result.content)
            : "—"}
        </>
      }
      meta={
        <>
          {canonical.result
            ? t(canonical.result.isError ? "toolResultError" : "toolResultSuccess")
            : "—"}
          {!tool.sourceModelRequestId ? ` · ${t("associationUnknown")}` : ""}
        </>
      }
      status={canonical.result?.isError ? "failed" : undefined}
      time={milliseconds(elapsed, i18n.language)}
      onClick={(e) =>
        onSelect(
          { kind: "tool", turnId: turn.identity.turnId, callId: tool.callId },
          e.currentTarget,
        )
      }
    />
  );
}
