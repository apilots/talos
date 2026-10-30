import { Copy, X } from "lucide-react";
import { Button } from "@/shared/ui/Button";
import { Select } from "@/shared/ui/Select";
import { TraceTabs } from "./TraceTabs";
import { useId } from "react";
import { useTranslation } from "react-i18next";
import {
  blockLabel,
  blockText,
  canonicalTool,
  type TraceSelection,
  type TraceTurn,
} from "../model/selectors";
import type { TraceTab, useSessionTracing } from "../hooks/useSessionTracing";
import { RequestParameters, RequestTraceDetail } from "./RequestTraceDetail";
import { ToolTraceView } from "./ToolTraceView";
export function TraceInspector({
  selection,
  turn,
  data,
  tab,
  onTab,
  onClose,
  onCallRecord,
  consumerId,
  onConsumer,
  rawField,
  onRawField,
  onCopy,
}: {
  selection: TraceSelection | null;
  turn?: TraceTurn;
  data: ReturnType<typeof useSessionTracing>;
  tab: TraceTab;
  onTab: (tab: TraceTab) => void;
  onClose: () => void;
  onCallRecord: (id: string) => void;
  consumerId: string;
  onConsumer: (id: string) => void;
  rawField: string;
  onRawField: (field: string) => void;
  onCopy: (text: string) => void;
}) {
  const { t } = useTranslation("tracing"),
    id = useId();
  if (!selection) return null;
  const tabs: TraceTab[] =
    selection.kind === "request"
      ? ["summary", "prompt", "tools", "response", "raw", "timing"]
      : selection.kind === "tool"
        ? ["arguments", "modelResult", "execution", "timing"]
        : [];
  const callRecord = data.callRecord,
    canonical =
      selection.kind === "tool" ? canonicalTool(turn, selection.callId) : null;
  const toolError = tab !== "execution" &&
    (data.tool.isError || data.request.isError || data.body.isError);
  const toolLoading = tab !== "execution" &&
    (data.tool.isFetching || data.request.isFetching || data.body.isFetching);
  return (
    <aside className="tracing-inspector" aria-label={t("record")}>
      <header>
        <div>
          <h2>
            {selection.kind === "fact"
              ? t(blockLabel(selection.block))
              : selection.kind === "tool"
                ? (canonical?.call?.name ??
                  canonical?.result?.toolName ??
                  selection.callId)
                : t("request", {
                    number: data.request.data?.request.ordinal ?? "—",
                  })}
          </h2>
          <p title={turn?.identity.turnId}>
            Turn {turn?.ordinal ?? "—"}
            {selection.kind === "request"
              ? ` · ${data.request.data?.request.modelId ?? "—"}`
              : null}
          </p>
        </div>
        <div className="tracing-inspector-actions">
          {selection.kind === "request" && (data.request.data?.request.callRecords.length ?? 0) > 1 ? (
            <Select
              aria-label={t("selectCallRecord")}
              className="tracing-select"
              value={callRecord?.callRecordId ?? ""}
              onValueChange={onCallRecord}
              options={(data.request.data?.request.callRecords ?? []).map((a) => ({
                value: a.callRecordId,
                label: `${t("callRecord", { number: a.ordinal })} · ${t(`states.${a.observedState}`)}`,
              }))}
            />
          ) : selection.kind === "request" && callRecord ? (
            <span className="tracing-call-state">
              {t("callRecord", { number: callRecord.ordinal })} · {t(`states.${callRecord.observedState}`)}
            </span>
          ) : null}
          <Button
            variant="ghost"
            className="tracing-close"
            data-tracing-close
            aria-label={t("close")}
            onClick={onClose}
          >
            <X aria-hidden size={16} />
          </Button>
        </div>
      </header>
      <TraceTabs
        label={t("record")}
        value={tab}
        onValueChange={onTab}
        tabs={tabs.map((tabId) => ({
          value: tabId,
          label: t(tabId),
          id: `${id}-${tabId}`,
          controls: `${id}-panel`,
        }))}
      />
      <div
        className="tracing-detail-body"
        id={`${id}-panel`}
        role={tabs.length ? "tabpanel" : undefined}
        aria-labelledby={tabs.length ? `${id}-${tab}` : undefined}
        tabIndex={0}
      >
        {selection.kind === "request" ? (
          <RequestParameters data={data} onCopy={onCopy} />
        ) : null}
        {selection.kind === "fact" ? (
          <>
            <div className="tracing-body-toolbar">
              <Button
                variant="secondary"
                className="tracing-action"
                onClick={() => onCopy(blockText(selection.block))}
              >
                <Copy aria-hidden size={14} />
                {t("copy")}
              </Button>
            </div>
            <pre>{blockText(selection.block)}</pre>
          </>
        ) : selection.kind === "tool" ? (
          <>
            {toolError ? (
              <p role="alert">
                {t("readError")}{" "}
                <Button variant="secondary" className="tracing-action" onClick={() => void data.retryRead()}>
                  {t("retry")}
                </Button>
              </p>
            ) : toolLoading ? (
              <p>{t("loading")}</p>
            ) : null}
            {!toolError && !toolLoading ? <ToolTraceView
              reference={data.bodyRef}
              turn={turn}
              callId={selection.callId}
              detail={data.tool.data ?? undefined}
              tab={tab}
              body={data.body.data}
              consumerId={consumerId}
              onConsumer={onConsumer}
              onCopy={onCopy}
            /> : null}
          </>
        ) : (
          <RequestTraceDetail
            data={data}
            tab={tab}
            messageIndex={selection.messageIndex}
            rawField={rawField}
            onRawField={onRawField}
            onCopy={onCopy}
          />
        )}
      </div>
    </aside>
  );
}
