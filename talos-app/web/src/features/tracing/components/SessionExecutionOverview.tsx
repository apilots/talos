import { useMemo, useState, useRef, useEffect, useCallback, type CSSProperties } from "react";
import { useTranslation } from "react-i18next";
import type { useSessionTracing } from "../hooks/useSessionTracing";
import type { TraceSelection, TraceTurn } from "../model/selectors";
import { ExecutionAxis, ExecutionBars, TimingLegend } from "./ExecutionClock";

export function SessionExecutionOverview({
  turns, timelines, selection, onSelect, onRetry,
}: {
  turns: TraceTurn[];
  timelines: ReturnType<typeof useSessionTracing>["timelines"];
  selection: TraceSelection | null;
  onSelect: (selection: TraceSelection, button: HTMLButtonElement) => void;
  onRetry: () => void;
}) {
  const { t } = useTranslation("tracing");
  const [compact, setCompact] = useState(true);
  const scroll = useRef<HTMLDivElement>(null);
  const lastInteractionTurn = useRef<string | null>(null);
  const [focusedIntervalTurn, setFocusedIntervalTurn] = useState<string | null>(null);
  const selectInterval = useCallback((next: TraceSelection, button: HTMLButtonElement) => {
    // Retain one trigger Turn so closing details can restore focus after scrolling.
    lastInteractionTurn.current = next.turnId;
    onSelect(next, button);
  }, [onSelect]);
  const [viewport, setViewport] = useState({ left: 0, width: 760 });
  const segments = useMemo(() => {
    const metadata = new Map(turns.map((turn) => [turn.identity.turnId, {
      ordinal: turn.ordinal,
      tools: turn.messages.flatMap((message) => message.blocks.flatMap((block) => block.content.type === "toolResult" ? [block.content.data] : [])),
    }]));
    let compactOffset = 0;
    const savedStart = Math.min(...timelines.flatMap((timeline) =>
      timeline.clocks.filter((clock) => Number.isFinite(clock.startedAt)).map((clock) => clock.startedAt * 1000),
    ));
    return timelines.flatMap((timeline) => {
      const clocks = timeline.clocks.filter((clock) => Number.isFinite(clock.startedAt));
      const turnStart = Math.min(...clocks.map((clock) => clock.startedAt * 1000));
      const turnSpan = Math.max(0, ...clocks.map((clock) =>
        clock.startedAt * 1000 - turnStart + clock.observedThroughOffsetMs,
      ));
      const result = clocks.map((clock, index) => ({
        ...timeline, clock, ordinal: metadata.get(timeline.turnId)?.ordinal,
        tools: (metadata.get(timeline.turnId)?.tools ?? []).filter((tool) => {
          const start = tool.metrics?.startedAtMs;
          return start != null && start >= clock.startedAt * 1000 &&
            (index === clocks.length - 1 || start < clocks[index + 1].startedAt * 1000);
        }),
        offsetMs: compact
          ? compactOffset + clock.startedAt * 1000 - turnStart
          : clock.startedAt * 1000 - savedStart,
      }));
      compactOffset += turnSpan;
      return result;
    });
  }, [timelines, turns, compact]);
  const span = Math.max(1, ...segments.map(({ clock, offsetMs }) =>
    offsetMs + clock.observedThroughOffsetMs,
  ));
  const calls = timelines.reduce((total, timeline) => total + timeline.entries.reduce((n, entry) =>
    n + (entry.type === "modelRequest" ? entry.request.callRecords.length : 0), 0), 0);
  useEffect(() => {
    const element = scroll.current;
    if (!element) return;
    const update = () => {
      if (element.clientWidth > 0) setViewport({ left: element.scrollLeft, width: element.clientWidth });
    };
    update();
    const observer = new ResizeObserver(update);
    observer.observe(element);
    return () => observer.disconnect();
  }, [segments.length]);
  const width = Math.max(viewport.width, calls * 32 + 56);
  const ticks = Math.max(4, Math.ceil((width - 56) / 200));
  // Keep one viewport of overscan; all metadata remains available for scrolling.
  const visibleSegments = segments.filter(({ offsetMs, clock, turnId }) =>
    turnId === selection?.turnId || turnId === lastInteractionTurn.current || turnId === focusedIntervalTurn || (
      offsetMs + clock.observedThroughOffsetMs >= (viewport.left - viewport.width) / (width - 56) * span &&
      offsetMs <= (viewport.left + 2 * viewport.width) / (width - 56) * span
    ),
  );
  return (
    <details className="tracing-overview tracing-session-overview" aria-label={t("sessionTimeline")} open>
      <summary className="tracing-section-heading">
        <h2>{t("overview")}<small>{t("sessionTimeline")}</small></h2>
        <span title={t("sessionClockPrecision")}>{t(compact ? "sessionCompactScope" : "sessionClockScope")}{width > viewport.width ? ` · ${t("scrollHint")}` : ""}</span>
      </summary>
      {timelines.some((timeline) => timeline.loading) ? <p className="tracing-field-note">{t("loading")}</p> : null}
      {timelines.some((timeline) => timeline.error) ? (
        <p role="alert">{t("readError")} <button type="button" onClick={onRetry}>{t("retry")}</button></p>
      ) : null}
      {segments.length ? (
        <>
          <label className="tracing-timeline-option" title={t("compactIdleHelp")}>
            <input type="checkbox" checked={compact} onChange={(event) => setCompact(event.currentTarget.checked)} />
            {t("compactIdle")}
          </label>
          <div ref={scroll} className="tracing-timeline-scroll" tabIndex={0}
            onFocusCapture={(event) => {
              const interval = (event.target as HTMLElement).closest<HTMLElement>("[data-tracing-turn-id]");
              setFocusedIntervalTurn(interval?.dataset.tracingTurnId ?? null);
            }}
            onBlurCapture={(event) => {
              if (!event.currentTarget.contains(event.relatedTarget as Node | null)) setFocusedIntervalTurn(null);
            }}
            onScroll={(event) => {
              const left = event.currentTarget.scrollLeft;
              setViewport((previous) => Math.abs(left - previous.left) >= previous.width / 2
                ? { ...previous, left } : previous);
            }} role="region" aria-label={t("timelineScroll")}>
          <div style={{ minWidth: width, "--trace-tick-step": `${100 / ticks}%` } as CSSProperties}>
          <ExecutionAxis spanMs={span} ticks={ticks} />
          {["inputAnchor", "llm", "tools"].map((lane) => (
            <div className="tracing-lane" key={lane}>
              <span>{lane === "inputAnchor" ? t("input") : t(lane)}</span>
              <div>{visibleSegments.map(({ turnId: id, ordinal, entries, tools, clock, offsetMs }) => (
                <ExecutionBars key={`${id}/${clock.segmentId}`} clock={clock}
                  turnId={id} entries={entries} tools={tools} lane={lane} selection={selection?.turnId === id ? selection : null}
                  onSelect={selectInterval}
                  offsetMs={offsetMs} spanMs={span}
                  labelPrefix={`Turn ${ordinal} · `} />
              ))}</div>
            </div>
          ))}
          </div>
          </div>
          <TimingLegend />
        </>
      ) : !timelines.some((timeline) => timeline.loading || timeline.error) ? (
        <p className="tracing-field-note">{t("noTiming")}</p>
      ) : null}
      {timelines.some((timeline) => !timeline.loading && !timeline.error && !timeline.clocks.length) ? (
        <p className="tracing-field-note">{t("missingTurnTiming")}</p>
      ) : null}
    </details>
  );
}
