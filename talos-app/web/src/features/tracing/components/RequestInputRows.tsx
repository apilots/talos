import { useTranslation } from "react-i18next";
import type { AppProtocol as API } from "@talos/sdk-core";
import { snapshotMessageKind, type TraceSelection } from "../model/selectors";
import { TraceRow } from "./TraceRow";
export function RequestInputRows({
  detail,
  payloads,
  turnId,
  modelRequestId,
  selection,
  onSelect,
}: {
  detail?: API.TrajectoryReadModelRequestResponse;
  payloads: Map<string, API.TrajectoryPayload>;
  turnId: string;
  modelRequestId: string;
  selection: TraceSelection | null;
  onSelect: (selection: TraceSelection, button: HTMLButtonElement) => void;
}) {
  const { t } = useTranslation("tracing");
  const prompt = detail?.prompt
    ? payloads.get(detail.prompt.payloadId)
    : undefined;
  const input =
    prompt?.kind === "prompt"
      ? prompt.messages.filter(
          (m) => m.role !== "assistant" && m.role !== "tool",
        )
      : [];
  const selected =
    selection?.kind === "request" &&
    selection.modelRequestId === modelRequestId &&
    selection.view === "prompt";
  return (
    <>
      {input.length ? (
        input.map((m) => {
          const kind = snapshotMessageKind(m);
          const tag = kind === "userContent" ? "USER" : kind.toUpperCase();
          return (
            <TraceRow
              key={m.index}
              kind={kind}
              tag={tag}
              title={
                m.inputOrigin && m.inputOrigin.source !== "unknown"
                  ? t(`inputSources.${m.inputOrigin.source}`)
                  : null
              }
              preview={m.content || t("knownEmpty")}
              selected={
                selected &&
                (selection.messageIndex ?? input[0].index) === m.index
              }
              onClick={(e) =>
                onSelect(
                  {
                    kind: "request",
                    turnId,
                    modelRequestId,
                    view: "prompt",
                    messageIndex: m.index,
                  },
                  e.currentTarget,
                )
              }
            />
          );
        })
      ) : (
        <TraceRow
          kind="input"
          tag="INPUT"
          title={t("prompt")}
          preview={prompt?.kind === "prompt" ? t("knownEmpty") : detail && !detail.prompt ? "—" : t("notLoaded")}
          selected={selected}
          onClick={(e) =>
            onSelect(
              { kind: "request", turnId, modelRequestId, view: "prompt" },
              e.currentTarget,
            )
          }
        />
      )}
    </>
  );
}
