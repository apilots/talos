import { Copy } from "lucide-react";
import { Button } from "@/shared/ui/Button";
import { Select } from "@/shared/ui/Select";
import { useTranslation } from "react-i18next";
import type { TraceTab, useSessionTracing } from "../hooks/useSessionTracing";
import { RequestSnapshotView } from "./RequestSnapshotView";
import { RequestMetricsView } from "./RequestMetricsView";
type TraceData = ReturnType<typeof useSessionTracing>;
export function RequestParameters({
  data,
  onCopy,
}: {
  data: TraceData;
  onCopy: (text: string) => void;
}) {
  const { t } = useTranslation("tracing");
  return (
    <details className="tracing-parameters">
      <summary>{t("parameters")}</summary>
      <RequestSnapshotView
        value={data.parameters.data}
        reference={data.request.data?.parameters}
        loading={data.parameters.isFetching}
        error={data.parameters.isError}
      />
      <Button
        variant="secondary"
        className="tracing-action"
        disabled={!data.parameters.data}
        onClick={() => onCopy(JSON.stringify(data.parameters.data, null, 2))}
      >
        <Copy aria-hidden size={14} />
        {t("copyParameters")}
      </Button>
    </details>
  );
}
export function RequestTraceDetail({
  data,
  tab,
  messageIndex,
  rawField,
  onRawField,
  onCopy,
}: {
  data: TraceData;
  tab: TraceTab;
  messageIndex?: number;
  rawField: string;
  onRawField: (field: string) => void;
  onCopy: (text: string) => void;
}) {
  const { t } = useTranslation("tracing");
  const snapshotCopy = () =>
    onCopy(
      JSON.stringify(
        tab === "summary" || tab === "timing"
          ? { request: data.request.data?.request, callRecord: data.callRecord }
          : data.body.data,
        null,
        2,
      ),
    );
  return (
    <>
      {data.request.isError ? <p role="alert">{t("readError")}</p> : null}
      <div className="tracing-body-toolbar">
        <Button
          variant="secondary"
          className="tracing-action"
          disabled={
            !data.request.data ||
            (!["summary", "timing"].includes(tab) && !data.body.data)
          }
          onClick={snapshotCopy}
        >
          <Copy aria-hidden size={14} />
          {t("copyRecord")}
        </Button>
      </div>
      {data.request.isError || data.parameters.isError || data.body.isError ? (
        <Button variant="secondary" className="tracing-action" onClick={() => void data.retryRead()}>{t("retry")}</Button>
      ) : null}
      {tab === "summary" || tab === "timing" ? (
        <>
          <RequestMetricsView
            detail={data.request.data}
            callRecord={data.callRecord}
            clocks={data.turn.data?.pages[0]?.clockSegments ?? []}
            timing={tab === "timing"}
          />
        </>
      ) : (
        <>
          {tab === "raw" ? (
            <Select
              aria-label={t("raw")}
              className="tracing-select"
              value={rawField}
              onValueChange={onRawField}
              options={["prompt", "tools", "parameters", "response"].map(
                (field) => ({ value: field, label: t(field) }),
              )}
            />
          ) : null}
          <RequestSnapshotView
            value={data.body.data}
            messageIndex={messageIndex}
            reference={data.bodyRef}
            error={data.body.isError}
            loading={data.body.isFetching}
            raw={tab === "raw"}
          />
        </>
      )}
    </>
  );
}
