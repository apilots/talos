import { useCallback } from "react";
import type { TraceSelection } from "../model/selectors";
import { useTranslation } from "react-i18next";
import { userVisibleErrorMessage } from "@/shared/i18n/apiErrorMessage";
import {
  useTracingWorkbench,
  type SessionTracingPanelProps,
} from "../hooks/useTracingWorkbench";
import { ObservedWorkSummary } from "./ObservedWorkSummary";
import { TurnMetricsSummary } from "./TurnMetricsSummary";
import { SessionExecutionOverview } from "./SessionExecutionOverview";
import { TurnTraceLedger } from "./TurnTraceLedger";
import { TraceInspector } from "./TraceInspector";
import { TraceToolbar } from "./TraceToolbar";
import "../tracing.css";
export type { SessionTracingPanelProps } from "../hooks/useTracingWorkbench";
export function SessionTracingPanel(props: SessionTracingPanelProps) {
  const { t } = useTranslation("tracing");
  const { sessionId, visible } = props;
  const {
    root,
    selection,
    ledgerSelection,
    turns,
    turn,
    turnId,
    data,
    tab,
    setTab,
    consumer,
    setConsumer,
    rawField,
    setRawField,
    copyStatus,
    range,
    mobileDetail,
    narrow,
    select,
    close,
    copy,
    copyTurn,
    selectTurn,
    changeRange,
    selectCallRecord,
  } = useTracingWorkbench(props);
  const selectInterval = useCallback((next: TraceSelection, button: HTMLButtonElement) => {
    changeRange("all");
    select(next, button);
    setTab("timing");
  }, [changeRange, select, setTab]);
  const error = data.capability.error ?? data.turn.error;
  const visibleTurns =
    range === "all"
      ? turns
      : turns.filter((item) => item.identity.turnId === range);
  return (
    <section
      ref={root}
      className={visible ? "tracing-root" : "hidden"}
      data-narrow={narrow}
      aria-label={t("title")}
      onKeyDown={(e) => {
        if (!e.defaultPrevented && e.key === "Escape" && selection) {
          e.preventDefault();
          close();
        }
      }}
    >
      <TraceToolbar
        sessionId={sessionId}
        turns={turns}
        turn={turn}
        range={range}
        onRange={changeRange}
      />
      <p
        className={copyStatus ? "tracing-copy-status" : "sr-only"}
        role="status"
      >
        {copyStatus}
      </p>
      {error ? (
        <p role="alert" className="text-danger">
          {userVisibleErrorMessage(error, t("readError"))}{" "}
          <button type="button" onClick={() => void data.retryRead()}>{t("retry")}</button>
        </p>
      ) : null}
      {data.capability.isPending && visible ? (
        <p>{t("loading")}</p>
      ) : data.capability.data?.supported === false ? (
        <p className="text-muted">{t("unavailable")}</p>
      ) : null}
      {!turn ? (
        <p>{t("empty")}</p>
      ) : (
        <>
          <div
            className={
              mobileDetail
                ? "tracing-overview-wrap mobile-detail"
                : "tracing-overview-wrap"
            }
          >
            <TurnMetricsSummary metrics={turn.metrics} ordinal={turn.ordinal} />
            {data.capability.data?.supported === true ? (
              <SessionExecutionOverview
                turns={turns} timelines={data.timelines} selection={ledgerSelection}
                onSelect={selectInterval}
                onRetry={() => void data.retryRead()}
              />
            ) : null}
            <ObservedWorkSummary
              ordinal={turn.ordinal}
              entries={data.entries}
            />
          </div>
          <div
            className={`tracing-workbench ${selection ? "has-detail" : ""} ${mobileDetail ? "mobile-detail" : ""}`}
          >
            <TurnTraceLedger
              selection={ledgerSelection}
              turns={visibleTurns}
              turnId={turnId}
              entries={data.entries}
              loading={data.capability.isPending || (data.capability.data?.supported === true && data.turn.isPending)}
              hasSupplement={data.turn.data?.pages[0]?.data != null}
              details={data.loadedRequests}
              payloads={data.loadedPayloads}
              onCopy={copyTurn}
              onTurn={selectTurn}
              onSelect={select}
            />
            <TraceInspector
              selection={selection}
              turn={turn}
              data={data}
              tab={tab}
              onTab={setTab}
              onClose={close}
              onCallRecord={selectCallRecord}
              consumerId={consumer}
              onConsumer={setConsumer}
              rawField={rawField}
              onRawField={setRawField}
              onCopy={(text) => {
                void copy(text);
              }}
            />
          </div>
        </>
      )}
    </section>
  );
}
