import { useTranslation } from "react-i18next";
import { Fragment } from "react";
import type { AppProtocol as API } from "@talos/sdk-core";
import { TraceBadge } from "./TraceRow";
import { snapshotMessageKind } from "../model/selectors";

/** Present the same recorded message metadata in Prompt and tool-result views. */
export function SnapshotMessageView({
  message: m,
  selected = false,
}: {
  message: API.SnapshotMessage;
  selected?: boolean;
}) {
  const { t } = useTranslation("tracing");
  const kind = snapshotMessageKind(m);
  const origin = m.inputOrigin;
  return (
    <section
      data-message-index={m.index}
      className={`tracing-message ${selected ? "is-current" : ""}`}
    >
      <header className="tracing-message-heading">
      <h3>
        <TraceBadge kind={kind}>
          {kind === "userContent" ? "USER" : kind.toUpperCase()}
        </TraceBadge>
        {m.name ? <span translate="no">{m.name}</span> : null}
        <span className="tracing-message-id" translate="no" title={`${t("messageRole")}: ${m.role}${m.toolCallId ? ` · callId: ${m.toolCallId}` : ""}`}>
          #{m.index}
        </span>
      </h3>
      {origin && origin.source !== "unknown" ? <span className="tracing-message-origin" title={t("sourceHelp")}>
        {t(`inputSources.${origin.source}`)}
        {origin.admission !== "unknown" ? ` · ${t(`inputAdmissions.${origin.admission}`)}` : ""}
      </span> : null}
      </header>
      {m.reasoning?.length ? (
        <details>
          <summary>{t("replayedReasoning")}</summary>
          {m.reasoning.map((part, index) => (
            <Fragment key={index}>
              {part.type === "summary" ? <small>{t("reasoningSummary")}</small> : null}
              <pre className="tracing-message-content">{part.text}</pre>
            </Fragment>
          ))}
        </details>
      ) : null}
      <pre className="tracing-message-content">{m.content}</pre>
      {m.toolCalls?.map((c) => (
        <details key={c.callId}>
          <summary>
            {c.name || "—"} · {c.callId || "—"}
          </summary>
          <h4>{t("structuredArguments")}</h4>
          <pre>{JSON.stringify(c.arguments, null, 2)}</pre>
          <h4>{t("originalArguments")}</h4>
          <pre>{c.argumentsText ?? t("rawMissing")}</pre>
        </details>
      ))}
      {m.attachments?.map((attachment, index) => (
        <dl className="tracing-timing" key={index}>
          {[
            [t("attachmentFile"), attachment.fileId ?? "—"],
            [t("attachmentName"), attachment.filename ?? "—"],
            [
              t("attachmentKind"),
              attachment.kind ? t(`attachmentKinds.${attachment.kind}`) : "—",
            ],
            [t("attachmentMime"), attachment.mimeType ?? "—"],
            [t("attachmentBytes"), attachment.sizeBytes ?? "—"],
            [t("status"), t(`attachmentAvailability.${attachment.availability}`)],
          ].map(([label, value]) => (
            <Fragment key={label}>
              <dt>{label}</dt>
              <dd>{value}</dd>
            </Fragment>
          ))}
        </dl>
      ))}
      {m.cacheControl?.type === "ephemeral" ? (
        <small className="text-muted">
          {t("cacheControl")}: {t(`cacheControls.${m.cacheControl.type}`)}
        </small>
      ) : null}
    </section>
  );
}
