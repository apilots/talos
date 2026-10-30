import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import type { AppProtocol as API } from "@talos/sdk-core";
import { type TraceSelection, type TraceTurn } from "../model/selectors";
import { milliseconds } from "../model/formatters";
import { TurnFeedbackRow } from "./TurnFeedbackRow";
import { RequestTraceRows } from "./RequestTraceRows";
import { ToolTraceRow } from "./ToolTraceRow";
import { Button } from "@/shared/ui/Button";
import { Copy } from "lucide-react";
import { apiErrorMessage } from "@/shared/i18n/apiErrorMessage";
export function TurnTraceLedger({
  selection,
  turns,
  turnId,
  entries,
  loading,
  hasSupplement,
  onTurn,
  onSelect,
  details,
  payloads,
  onCopy,
}: {
  selection: TraceSelection | null;
  turns: TraceTurn[];
  turnId?: string;
  entries: API.TrajectoryEntry[];
  loading: boolean;
  hasSupplement: boolean;
  onTurn: (id: string) => void;
  onSelect: (selection: TraceSelection, button: HTMLButtonElement) => void;
  details: API.TrajectoryReadModelRequestResponse[];
  payloads: Map<string, API.TrajectoryPayload>;
  onCopy: () => void;
}) {
  const { t, i18n } = useTranslation("tracing");
  const durations = useMemo(() => new Map(turns.map((turn) => [
    turn.identity.turnId, milliseconds(turn.metrics?.durationMs, i18n.language),
  ])), [turns, i18n.language]);
  const ordinal = turns.find(
    (turn) => turn.identity.turnId === turnId,
  )?.ordinal;
  const requests = entries.filter(
    (entry) => entry.type === "modelRequest",
  ).length;
  const associatedTools = new Set(entries.flatMap((entry) =>
    entry.type === "tool" && entries.some((parent) =>
      parent.type === "modelRequest" &&
      parent.request.modelRequestId === entry.tool.sourceModelRequestId &&
      parent.request.callRecords.some((callRecord) => callRecord.callRecordId === entry.tool.sourceCallRecordId),
    ) ? [entry.tool.callId] : [],
  ));
  const observedTools = new Set(entries.flatMap((entry) =>
    entry.type === "tool" ? [entry.tool.callId] : [],
  ));
  return (
    <section className="tracing-ledger" aria-label={t("ledger")}>
      <header className="tracing-ledger-toolbar">
        <div>
          <h2>{t("ledger")}</h2>
          <small>
            Turn {ordinal ?? "—"} ·{" "}
            {t("loadedRequests", { count: requests })}
          </small>
        </div>
        <Button variant="secondary" className="tracing-action" onClick={onCopy}>
          <Copy aria-hidden="true" size={14} />
          {t("copyTurn", { number: ordinal ?? "—" })}
        </Button>
      </header>
      <div
        className="tracing-ledger-list"
        tabIndex={0}
        aria-label={t("continuousRecords")}
      >
        {turns.map((turn) => {
          const active = turn.identity.turnId === turnId;
          const feedback = active ? turn.messages.flatMap((message) =>
            message.blocks.filter((block) => !(
              (block.content.type === "toolCall" || block.content.type === "toolResult") &&
              observedTools.has(block.content.data.callId)
            )).map((block) => (
              <TurnFeedbackRow
                key={`${message.messageId}/${block.blockId}`}
                block={block}
                turnId={turn.identity.turnId}
                onSelect={onSelect}
                selected={
                  (selection?.kind === "fact" && selection.block.blockId === block.blockId) ||
                  (selection?.kind === "tool" &&
                    (block.content.type === "toolCall" || block.content.type === "toolResult") &&
                    block.content.data.callId === selection.callId)
                }
              />
            )),
          ) : [];
          return <details
            className="tracing-turn"
            key={turn.identity.turnId}
            open={active}
            onToggle={(e) => {
              if (e.currentTarget.open && !active)
                onTurn(turn.identity.turnId);
            }}
          >
            <summary>
              <strong>Turn {turn.ordinal}</strong>
              <span>
                {t(`states.${turn.status}`)}
              </span>
              <span className="tracing-turn-duration">
                {t("duration")}
                <span>
                  {durations.get(turn.identity.turnId)}
                </span>
              </span>
            </summary>
            {active ? <>
            {turn.result ? (
              <p className="tracing-turn-outcome">
                {t("turnResult")}: {turn.result.status === "completed"
                  ? t(`turnStopReasons.${turn.result.result.completion.stopReason}`)
                  : apiErrorMessage(turn.result.result.failure.error)}
              </p>
            ) : null}
            <details className="tracing-feedback" open>
              <summary>
                {t("feedback")}
                <small>
                  {t("order")} ·{" "}
                  {feedback.length}
                </small>
              </summary>
              {feedback}
            </details>
            {!entries.length ? (
              <p className="tracing-gap">
                {t(loading ? "loading" : hasSupplement ? "knownEmpty" : "noDetails")}
              </p>
            ) : null}
            {entries.map((entry) =>
              entry.type === "modelRequest" ? (
                <RequestTraceRows
                  key={entry.request.modelRequestId}
                  request={entry.request}
                  detail={details.find(
                    (d) => d.request.modelRequestId === entry.request.modelRequestId,
                  )}
                  payloads={payloads}
                  turn={turn}
                  entries={entries}
                  selection={selection}
                  onSelect={onSelect}
                />
              ) : null,
            )}
            {entries.flatMap((e) =>
              e.type === "tool" &&
              !associatedTools.has(e.tool.callId)
                ? [
                    <ToolTraceRow
                      key={e.tool.callId}
                      tool={e.tool}
                      turn={turn}
                      selection={selection}
                      onSelect={onSelect}
                    />,
                  ]
                : [],
            )}
            </> : null}
          </details>
        })}
      </div>
      <footer>{t("ledgerScope")}</footer>
    </section>
  );
}
