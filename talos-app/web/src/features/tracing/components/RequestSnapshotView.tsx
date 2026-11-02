import { useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import type { AppProtocol as API } from "@talos/sdk-core";
import { SnapshotMessageView } from "./SnapshotMessageView";
export function RequestSnapshotView({
  value,
  reference,
  error,
  loading,
  raw = false,
  messageIndex,
}: {
  value: API.TrajectoryPayload | null | undefined;
  reference?: API.PayloadRef | null;
  error?: boolean;
  loading?: boolean;
  raw?: boolean;
  messageIndex?: number;
}) {
  const { t } = useTranslation("tracing");
  const root = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (messageIndex != null && value?.kind === "prompt" && !raw)
      root.current
        ?.querySelector(`[data-message-index="${messageIndex}"]`)
        ?.scrollIntoView({ block: "start" });
  }, [messageIndex, value, raw]);
  return (
    <div ref={root}>
      {error ? (
        <p role="alert">{t("readError")}</p>
      ) : loading ? (
        <p>{t("loading")}</p>
      ) : null}
      {reference && reference.complete !== true ? (
        <p className="tracing-field-note">
          {reference.complete === false ? t("incompleteContent") : `${t("contentCompleteness")}: —`}
        </p>
      ) : null}
      {!value ? (
        !loading && !error ? (
          <p className="text-muted">{reference ? t("notLoaded") : "—"}</p>
        ) : null
      ) : raw ? (
        <pre>{JSON.stringify(value, null, 2)}</pre>
      ) : value.kind === "prompt" ? (
        <div>
          {!value.messages.length ? <p>{t("knownEmpty")}</p> : null}
          {value.messages.map((m) => (
            <SnapshotMessageView
              key={m.index}
              message={m}
              selected={messageIndex === m.index}
            />
          ))}
          {value.messages.some((m) => m.inputOrigin == null || m.inputOrigin.source === "unknown") ? (
            <p className="tracing-field-note">{t("sourceUnknownHelp")}</p>
          ) : null}
        </div>
      ) : value.kind === "response" ? (
        <>
          {value.parts.length === 0 ? (
            <p>{t("knownEmpty")}</p>
          ) : (
            value.parts.map((part, index) => (
              <section key={index}>
                <h3>
                  {part.type === "reasoning"
                    ? t("reasoning")
                    : part.type === "text"
                      ? t("response")
                      : part.call.name || "—"}
                </h3>
                {part.type === "toolCall" ? (
                  <small translate="no">{part.call.callId || "—"}</small>
                ) : null}
                <pre>
                  {part.type === "toolCall"
                    ? JSON.stringify(part.call.arguments, null, 2)
                    : part.text}
                </pre>
                {part.type === "toolCall" ? (
                  <>
                    <h4>{t("originalArguments")}</h4>
                    <pre>{part.call.argumentsText ?? t("rawMissing")}</pre>
                  </>
                ) : null}
              </section>
            ))
          )}
        </>
      ) : value.kind === "preparedResult" ? (
        <SnapshotMessageView message={value.message} />
      ) : value.kind === "tools" ? (
        <>
          {!value.definitions.length ? (
            <p>{t("knownEmpty")}</p>
          ) : (
            value.definitions.map((d) => (
              <details key={d.name}>
                <summary>{d.name}</summary>
                <pre>{d.description}</pre>
                <pre>{JSON.stringify(d.parameters, null, 2)}</pre>
              </details>
            ))
          )}
        </>
      ) : (
        <pre>{JSON.stringify(value.options, null, 2)}</pre>
      )}
    </div>
  );
}
