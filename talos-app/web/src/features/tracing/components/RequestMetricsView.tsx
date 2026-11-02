import { useTranslation } from "react-i18next";
import type { AppProtocol as API } from "@talos/sdk-core";
import { apiErrorMessage } from "@/shared/i18n/apiErrorMessage";
import { milliseconds, number } from "../model/formatters";
export function RequestMetricsView({
  detail,
  callRecord,
  clocks,
  timing = false,
}: {
  detail?: API.TrajectoryReadModelRequestResponse;
  callRecord?: API.ModelCallRecord;
  clocks: API.ClockSegment[];
  timing?: boolean;
}) {
  const { t, i18n } = useTranslation("tracing"),
    clock = clocks.find(
      (c) => c.segmentId === callRecord?.metrics?.segmentId,
    );
  const statistics = [
    [t("llm"), callRecord?.metrics?.executionDurationMs],
    [t("ttft"), callRecord?.metrics?.ttftMs],
    [t("generation"), callRecord?.metrics?.generationMs],
  ] as const;
  const offset = (value: number | null | undefined) =>
    value == null ? "—" : new Date(value).toLocaleString(i18n.language);
  const rows = timing
    ? [
        [
          t("dispatchOffset"),
          offset(callRecord?.metrics?.startedAtMs),
        ],
        [t("firstOutput"), offset(callRecord?.metrics?.firstOutputAtMs)],
        [t("lastOutput"), offset(callRecord?.metrics?.lastOutputAtMs)],
        [t("callEnd"), offset(callRecord?.metrics?.finishedAtMs)],
        [t("node"), clock?.sourceNodeId ?? "—"],
        [t("segment"), callRecord?.metrics?.segmentId ?? "—"],
        [
          t("clockStartedAt"),
          clock?.startedAt == null
            ? "—"
            : new Date(clock.startedAt * 1000).toLocaleString(i18n.language),
        ],
      ]
    : [
        [
          t("modelConnection"),
          `${detail?.request.modelId ?? "—"} / ${detail?.request.connectionId ?? "—"}`,
        ],
        [
          t("purposeStatus"),
          `${detail?.request.purpose ? t(`purposes.${detail.request.purpose}`) : "—"} / ${callRecord ? t(`states.${callRecord.observedState}`) : "—"}`,
        ],
        [t("stopReason"), callRecord?.finishReason ?? "—"],
      ];
  return (
    <>
      <dl className="tracing-statistics">
        {statistics.map(([label, value]) => (
          <div key={label}>
            <dt>{label}</dt>
            <dd>{milliseconds(value, i18n.language)}</dd>
          </div>
        ))}
      </dl>
      <div className="tracing-usage">
        {[
          [t("input"), callRecord?.metrics?.usage?.input_tokens],
          [t("output"), callRecord?.metrics?.usage?.output_tokens],
          [t("total"), callRecord?.metrics?.usage?.total_tokens],
        ].map(([label, value]) => (
          <span key={label}>
            {label}{" "}
            <b>{number(value as number | null | undefined, i18n.language)}</b>
          </span>
        ))}
        <span>tok</span>
      </div>
      <table className="tracing-timing-table">
        <tbody>
          {rows.map(([label, value]) => (
            <tr key={label}>
              <th scope="row">{label}</th>
              <td>{value}</td>
            </tr>
          ))}
        </tbody>
      </table>
      <details className="tracing-identifiers">
        <summary>{t("identifiers")}</summary>
        <p className="tracing-field-note">{t("identifierHelp")}</p>
        <dl className="tracing-timing">
          <dt>{t("modelRequestId")}</dt>
          <dd><code>{detail?.request.modelRequestId ?? "—"}</code></dd>
          <dt>{t("callRecordId")}</dt>
          <dd><code>{callRecord?.callRecordId ?? "—"}</code></dd>
        </dl>
      </details>
      {callRecord?.error ? (
        <p className="tracing-error">
          {callRecord.error.code} · {apiErrorMessage(callRecord.error)}
        </p>
      ) : null}
      <p className="tracing-field-note">
        {t(timing ? "timingScope" : "usageScope")}
      </p>
      <div className="tracing-usage">
        <span>
          {t("cacheRead")}{" "}
          <b>
            {number(callRecord?.metrics?.usage?.cache_read_input_tokens, i18n.language)}
          </b>
        </span>
        <span>
          {t("cacheWrite")}{" "}
          <b>
            {number(callRecord?.metrics?.usage?.cache_creation_input_tokens, i18n.language)}
          </b>
        </span>
      </div>
    </>
  );
}
