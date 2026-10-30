import { Copy } from "lucide-react";
import { Button } from "@/shared/ui/Button";
import { Select } from "@/shared/ui/Select";
import { useTranslation } from "react-i18next";
import type { AppProtocol as API } from "@talos/sdk-core";
import {
  canonicalTool,
  toolArgumentsText,
  type TraceTurn,
} from "../model/selectors";
import type { TraceTab } from "../hooks/useSessionTracing";
import { RequestSnapshotView } from "./RequestSnapshotView";
import { milliseconds } from "../model/formatters";
import { SnapshotMessageView } from "./SnapshotMessageView";
export function ToolTraceView({
  reference,
  turn,
  callId,
  detail,
  tab,
  body,
  consumerId,
  onConsumer,
  onCopy,
}: {
  reference?: API.PayloadRef | null;
  turn?: TraceTurn;
  callId: string;
  detail?: API.TrajectoryReadToolResponse;
  tab: TraceTab;
  body: API.TrajectoryPayload | null | undefined;
  consumerId: string;
  onConsumer: (id: string) => void;
  onCopy: (text: string) => void;
}) {
  const { t, i18n } = useTranslation("tracing"),
    canonical = canonicalTool(turn, callId);
  const rawCall =
    body?.kind === "response"
      ? body.parts
          .filter((part) => part.type === "toolCall")
          .find((part) => part.call.callId === callId)?.call
      : undefined;
  const association = (
    <details className="tracing-identifiers">
      <summary>{t("identifiers")}</summary>
      <dl className="tracing-timing">
      <dt>{t("sourceModelRequest")}</dt>
      <dd><code>{detail?.tool.sourceModelRequestId ?? "—"}</code></dd>
      <dt>{t("sourceCallRecord")}</dt>
      <dd><code>{detail?.tool.sourceCallRecordId ?? "—"}</code></dd>
      </dl>
    </details>
  );
  if (tab === "arguments")
    return (
      <>
        {association}
        <h3>{t("structuredArguments")}</h3>
        <pre>
          {canonical.call
            ? toolArgumentsText(canonical.call.arguments)
            : rawCall
              ? JSON.stringify(rawCall.arguments, null, 2)
              : "—"}
        </pre>
        <h3>{t("originalArguments")}</h3>
        {reference && reference.complete !== true ? <p className="tracing-field-note">{reference.complete === false ? t("incompleteContent") : `${t("contentCompleteness")}: —`}</p> : null}
        <p className="text-xs text-muted">{t("exactRaw")}</p>
        <pre>{rawCall?.argumentsText ?? t("rawMissing")}</pre>
        <Button
          variant="secondary"
          className="tracing-action"
          disabled={rawCall?.argumentsText == null}
          onClick={() =>
            onCopy(
              reference?.complete !== false
                ? rawCall!.argumentsText!
                : `[${t("incompleteContent")}]\n${rawCall!.argumentsText!}`,
            )
          }
        >
          <Copy aria-hidden size={14} />
          {t("copy")}
        </Button>
      </>
    );
  if (tab === "execution")
    return (
      <>
        <h3>{t("progress")}</h3>
        <pre>
          {canonical.call?.progress
            .map((p) => `[${p.kind} · ${p.occurredAt}] ${p.text}`)
            .join("\n") || "—"}
        </pre>
        <h3>{t("executionResult")}</h3>
        <p data-status={canonical.result?.isError ? "failed" : undefined}>
          {t("status")}: {canonical.result
            ? t(
                canonical.result.isError
                  ? "toolResultError"
                  : "toolResultSuccess",
              )
            : "—"}
        </p>
        <pre>
          {canonical.result?.content
            .map((c) =>
              c.type === "text" ? c.text : JSON.stringify(c, null, 2),
            )
            .join("\n") ?? "—"}
        </pre>
      </>
    );
  if (tab === "timing")
    return (
      <>
        {association}
        {detail?.tool.intervals?.length ? (
          detail.tool.intervals.map((p, index) => (
            <dl className="tracing-timing" key={index}>
              <dt>{t(p.kind)}</dt>
              <dd>{milliseconds(p.interval.elapsedMs, i18n.language)}</dd>
              <dt>{t("segment")}</dt>
              <dd>{p.interval.segmentId}</dd>
              <dt>{t("intervalOffsets")}</dt>
              <dd>
                +{milliseconds(p.interval.startedOffsetMs, i18n.language)} →{" "}
                {p.interval.endedOffsetMs == null
                  ? "—"
                  : `+${milliseconds(p.interval.endedOffsetMs, i18n.language)}`}
              </dd>
            </dl>
          ))
        ) : (
          <p>{t("noTiming")}</p>
        )}
      </>
    );
  const usage = detail?.usages?.find((u) => u.modelRequestId === consumerId);
  const message =
    body?.kind === "prompt"
      ? body.messages.find((m) => m.index === usage?.messageIndex)
      : null;
  const mismatch =
    message && (message.role !== "tool" || message.toolCallId !== callId);
  return (
    <>
      <label className="tracing-consumer">
        {t("consumed")}
        <Select
          aria-label={t("consumed")}
          className="tracing-select"
          value={consumerId}
          onValueChange={onConsumer}
          options={[
            { value: "prepared", label: t("prepared") },
            ...(detail?.usages ?? []).map((u) => ({
              value: u.modelRequestId,
              label: `${u.modelRequestId} · ${t(`toolUsages.${u.usage}`)}`,
            })),
          ]}
        />
      </label>
      {consumerId === "prepared" ? (
        <>
          <p className="text-xs text-muted">{t("preparedScope")}</p>
          <RequestSnapshotView
            value={body}
            reference={detail?.preparedResult}
          />
        </>
      ) : usage?.usage === "notIncluded" ? (
        <p>{t("notIncluded")}</p>
      ) : mismatch ? (
        <p role="alert">{t("invalidLink")}</p>
      ) : usage?.usage === "unknown" ? (
        <>
          <p>{t("unknownUsage")}</p>
          {message ? <SnapshotMessageView message={message} /> : null}
        </>
      ) : message ? (
        <SnapshotMessageView message={message} />
      ) : (
        <p>{t("notLoaded")}</p>
      )}
    </>
  );
}
