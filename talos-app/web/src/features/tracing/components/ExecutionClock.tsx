import { memo } from "react";
import { useTranslation } from "react-i18next";
import type { AppProtocol as API } from "@talos/sdk-core";
import { milliseconds, number } from "../model/formatters";
import type { TraceSelection } from "../model/selectors";

export const ExecutionAxis = memo(function ExecutionAxis({ spanMs, ticks = 4 }: { spanMs: number; ticks?: number }) {
  const { t, i18n } = useTranslation("tracing");
  const span = Math.max(1, spanMs);
  return (
    <div className="tracing-axis">
      <span>{t("seconds")}</span>
      <div>
        {Array.from({ length: ticks + 1 }, (_, index) => index / ticks).map((ratio) => (
          <span key={ratio} style={{ left: `${ratio * 100}%` }}>
            {number(
              Number(((span * ratio) / 1000).toFixed(2)),
              i18n.language,
            )}
          </span>
        ))}
      </div>
    </div>
  );
});

/** Draw task-owned Metrics on the existing Session axis. */
export const ExecutionBars = memo(function ExecutionBars({
  clock, turnId, entries, tools = [], lane, selection, onSelect,
  offsetMs = 0, spanMs, labelPrefix = "",
}: {
  clock: API.ClockSegment;
  turnId: string;
  entries: API.TrajectoryEntry[];
  tools?: API.AgentToolResult[];
  lane: string;
  selection: TraceSelection | null;
  onSelect: (selection: TraceSelection, button: HTMLButtonElement) => void;
  offsetMs?: number;
  spanMs?: number;
  labelPrefix?: string;
}) {
  const { t, i18n } = useTranslation("tracing");
  const span = Math.max(1, spanMs ?? clock.observedThroughOffsetMs);
  const origin = clock.startedAt * 1000;
  type Bar = { key: string; start: number; end: number | null; elapsed: number | null;
    first?: number | null; kind: string; failed?: boolean; label: string; selected: boolean; target: TraceSelection };
  const bars: Bar[] = lane === "tools"
    ? tools.flatMap((tool) => {
        const m = tool.metrics;
        if (m?.startedAtMs == null) return [];
        const target: TraceSelection = { kind: "tool", turnId, callId: tool.callId };
        const selected = selection?.kind === "tool" && selection.turnId === turnId && selection.callId === tool.callId;
        const execution: Bar = { key: tool.callId, start: m.startedAtMs, end: m.finishedAtMs,
          elapsed: m.executionDurationMs, kind: "tool", label: tool.callId, selected, target };
        return m.queuedAtMs == null || !m.queueDurationMs ? [execution] : [
          { ...execution, key: `${tool.callId}/queue`, start: m.queuedAtMs, end: m.startedAtMs,
            elapsed: m.queueDurationMs, kind: "waiting" }, execution,
        ];
      })
    : entries.flatMap((entry) => entry.type !== "modelRequest" ? [] : entry.request.callRecords.flatMap((call) => {
        const m = call.metrics;
        if (m?.startedAtMs == null || m.segmentId !== clock.segmentId) return [];
        return [{ key: `${entry.request.modelRequestId}/${call.callRecordId}`, start: m.startedAtMs, end: m.finishedAtMs,
          elapsed: m.executionDurationMs, first: m.firstOutputAtMs,
          kind: lane === "inputAnchor" ? "anchor" : "model", failed: call.observedState === "failed",
          label: `${t("request", { number: entry.request.ordinal })} · ${t("callRecord", { number: call.ordinal })}`,
          selected: selection?.kind === "request" && selection.turnId === turnId && selection.modelRequestId === entry.request.modelRequestId && selection.callRecordId === call.callRecordId,
          target: { kind: "request", turnId, modelRequestId: entry.request.modelRequestId, callRecordId: call.callRecordId } as TraceSelection,
        }];
      }));
  return <>{bars.map((bar) => (
    <button key={bar.key}
      className={`tracing-bar ${bar.kind} ${bar.failed ? "failed" : ""}`}
      style={{ left: `${Math.max(0, ((offsetMs + bar.start - origin) / span) * 100)}%`,
        width: lane === "inputAnchor" || bar.end == null ? "3px" : `${Math.max(0, Math.min(100, (bar.end - bar.start) / span * 100))}%` }}
      title={`${labelPrefix}${bar.label} · ${milliseconds(bar.elapsed, i18n.language)}${bar.end == null ? ` · ${t("endUnknown")}` : ""}`}
      aria-label={`${labelPrefix}${t(lane)} · ${bar.label}${bar.end == null ? ` · ${t("endUnknown")}` : ""}`}
      data-tracing-turn-id={turnId} aria-pressed={bar.selected}
      onClick={(event) => onSelect(bar.target, event.currentTarget)}>
      {lane === "llm" && bar.end != null ? <span className="tracing-preoutput" style={{ width: `${bar.first == null ? 100 : Math.max(0, Math.min(100, (bar.first - bar.start) / Math.max(1, bar.end - bar.start) * 100))}%` }} /> : null}
    </button>
  ))}</>;
});

export function TimingLegend() {
  const { t } = useTranslation("tracing");
  return (
    <div className="tracing-legend">
      {["anchor", "preoutput", "generation", "tool", "waiting"].map((kind) => (
        <span key={kind}>
          <i aria-hidden="true" className={kind} />
          {t(`legend.${kind}`)}
        </span>
      ))}
      <span>{t("anchorScope")}</span>
    </div>
  );
}
