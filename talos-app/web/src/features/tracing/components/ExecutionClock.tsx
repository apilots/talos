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

/** Place execution-side intervals on the Session overview axis. */
export const ExecutionIntervals = memo(function ExecutionIntervals({
  clock, turnId, entries, lane, selection, onSelect,
  offsetMs = 0, spanMs, labelPrefix = "",
}: {
  clock: API.ClockSegment;
  turnId: string;
  entries: API.TrajectoryEntry[];
  lane: string;
  selection: TraceSelection | null;
  onSelect: (selection: TraceSelection, button: HTMLButtonElement) => void;
  offsetMs?: number;
  spanMs?: number;
  labelPrefix?: string;
}) {
  const { t, i18n } = useTranslation("tracing");
  const span = Math.max(1, spanMs ?? clock.observedThroughOffsetMs);
  return (
    <>
      {entries.flatMap((entry) =>
        entry.type === "modelRequest"
          ? entry.request.callRecords.flatMap((a) => {
              const interval = a.timing?.interval;
              if (
                !interval ||
                interval.segmentId !== clock.segmentId ||
                lane === "tools"
              )
                return [];
              const start = interval.startedOffsetMs,
                end = interval.endedOffsetMs,
                first = a.timing?.firstOutputOffsetMs;
              const wait =
                first == null
                  ? 100
                  : Math.max(
                      0,
                      Math.min(
                        100,
                        ((first - start) / Math.max(1, (end ?? start) - start)) *
                          100,
                      ),
                    );
              return [
                <button
                  key={`${entry.request.modelRequestId}/${a.callRecordId}`}
                  className={`tracing-bar ${lane === "inputAnchor" ? "anchor" : "model"} ${a.observedState === "failed" ? "failed" : ""}`}
                  style={{
                    left: `${Math.max(0, ((offsetMs + start) / span) * 100)}%`,
                    width:
                      lane === "inputAnchor" || end == null
                        ? "3px"
                        : `${Math.max(0, Math.min(100, ((end - start) / span) * 100))}%`,
                  }}
                  title={`${labelPrefix}${t("request", { number: entry.request.ordinal })} · ${milliseconds(interval.elapsedMs, i18n.language)} · TTFT ${milliseconds(a.timing?.ttftMs, i18n.language)}${end == null ? ` · ${t("endUnknown")}` : ""}`}
                  aria-label={`${labelPrefix}${t(lane)} · ${t("request", { number: entry.request.ordinal })} · ${t("callRecord", { number: a.ordinal })}${end == null ? ` · ${t("endUnknown")}` : ""}`}
                  data-tracing-turn-id={turnId}
                  aria-pressed={
                    (selection?.turnId === turnId && selection.kind === "request" &&
                    selection.modelRequestId === entry.request.modelRequestId &&
                    selection.callRecordId === a.callRecordId)
                  }
                  onClick={(e) =>
                    onSelect(
                      {
                        kind: "request",
                        turnId,
                        modelRequestId: entry.request.modelRequestId,
                        callRecordId: a.callRecordId,
                      },
                      e.currentTarget,
                    )
                  }
                >
                  {lane === "llm" && end != null ? (
                    <span
                      className="tracing-preoutput"
                      style={{ width: `${wait}%` }}
                    />
                  ) : null}
                </button>,
              ];
            })
          : lane !== "tools"
            ? []
            : (entry.tool.intervals ?? [])
                .filter((p) => p.interval.segmentId === clock.segmentId)
                .map((phase, index) => (
                  <button
                    key={`${entry.tool.callId}/${index}`}
                    className={`tracing-bar ${phase.kind === "execution" ? "tool" : "waiting"}`}
                    style={{
                      left: `${Math.max(0, ((offsetMs + phase.interval.startedOffsetMs) / span) * 100)}%`,
                      width: phase.interval.endedOffsetMs == null ? "3px" : `${Math.max(0, Math.min(100, ((phase.interval.endedOffsetMs - phase.interval.startedOffsetMs) / span) * 100))}%`,
                    }}
                    title={`${labelPrefix}${entry.tool.callId} · ${t(phase.kind)} · ${milliseconds(phase.interval.elapsedMs, i18n.language)}${phase.interval.endedOffsetMs == null ? ` · ${t("endUnknown")}` : ""}`}
                    aria-label={`${labelPrefix}${entry.tool.callId} · ${t(phase.kind)}${phase.interval.endedOffsetMs == null ? ` · ${t("endUnknown")}` : ""}`}
                    data-tracing-turn-id={turnId}
                  aria-pressed={
                      (selection?.turnId === turnId && selection.kind === "tool" &&
                      selection.callId === entry.tool.callId)
                    }
                    onClick={(e) =>
                      onSelect(
                        {
                          kind: "tool",
                          turnId,
                          callId: entry.tool.callId,
                        },
                        e.currentTarget,
                      )
                    }
                  />
                )),
      )}
    </>
  );
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
