import { useTranslation } from "react-i18next";
import type { AppProtocol as API } from "@talos/sdk-core";
import { milliseconds, number } from "../model/formatters";
import type { TraceSelection, TraceTurn } from "../model/selectors";
import { RequestInputRows } from "./RequestInputRows";
import { ToolTraceRow } from "./ToolTraceRow";
import { TraceRow } from "./TraceRow";
export function RequestTraceRows({
  request,
  detail,
  payloads,
  turn,
  entries,
  selection,
  onSelect,
}: {
  request: API.ModelRequestTrace;
  detail?: API.TrajectoryReadModelRequestResponse;
  payloads: Map<string, API.TrajectoryPayload>;
  turn: TraceTurn;
  entries: API.TrajectoryEntry[];
  selection: TraceSelection | null;
  onSelect: (selection: TraceSelection, button: HTMLButtonElement) => void;
}) {
  const { t, i18n } = useTranslation("tracing");
  const turnId = turn.identity.turnId;
  const selected =
    selection?.kind === "request" && selection.modelRequestId === request.modelRequestId;
  return (
    <section>
      <h3 className="tracing-request-heading">
        <strong>{t("request", { number: request.ordinal })}</strong>
        <span>
          {t("callRecords", { count: request.callRecords.length })}{" "}
        </span>
      </h3>
      <RequestInputRows
        detail={detail}
        payloads={payloads}
        turnId={turnId}
        modelRequestId={request.modelRequestId}
        selection={selection}
        onSelect={onSelect}
      />
      {request.callRecords.map((a) => {
        const ref = detail?.outputs.find(
          (o) => o.callRecordId === a.callRecordId,
        )?.response;
        const value = ref ? payloads.get(ref.payloadId) : null;
        const response =
          value?.kind === "response"
            ? value.parts
                .map((p) =>
                  p.type === "toolCall" ? `[${p.call.name}]` : p.text,
                )
                .join("\n") || t("knownEmpty")
            : detail && !ref ? "—" : t("notLoaded");
        const attemptSelected = selected && selection.callRecordId === a.callRecordId;
        return (
          <section key={a.callRecordId}>
            <TraceRow
              kind="llm"
              tag="LLM"
              selected={attemptSelected && !selection.view}
              title={`${request.modelId} · ${t("callRecord", { number: a.ordinal })}`}
              preview={`${request.connectionId} · ${t(`purposes.${request.purpose}`)}`}
              meta={t(`states.${a.observedState}`)}
              status={a.observedState}
              time={
                <>
                  {milliseconds(a.metrics?.executionDurationMs, i18n.language)}
                  <small>
                    {number(a.metrics?.usage?.input_tokens, i18n.language)} /{" "}
                    {number(a.metrics?.usage?.output_tokens, i18n.language)}
                    <br />
                    {t("tokenInOut")}
                  </small>
                </>
              }
              onClick={(e) =>
                onSelect(
                  {
                    kind: "request",
                    turnId,
                    modelRequestId: request.modelRequestId,
                    callRecordId: a.callRecordId,
                  },
                  e.currentTarget,
                )
              }
            />
            <TraceRow
              kind="assistantText"
              tag="ASSISTANT"
              selected={attemptSelected && selection.view === "response"}
              title={t("response")}
              preview={response}
              onClick={(e) =>
                onSelect(
                  {
                    kind: "request",
                    turnId,
                    modelRequestId: request.modelRequestId,
                    callRecordId: a.callRecordId,
                    view: "response",
                  },
                  e.currentTarget,
                )
              }
            />
            {entries.flatMap((e) =>
              e.type === "tool" &&
              e.tool.sourceModelRequestId === request.modelRequestId &&
              e.tool.sourceCallRecordId === a.callRecordId
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
          </section>
        );
      })}
    </section>
  );
}
