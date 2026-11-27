import { useEffect, useRef, useState } from "react";
import { Bot, Check, Circle, Loader2 } from "lucide-react";
import { useTranslation } from "react-i18next";

import { useCanUseTalo } from "@/features/auth";
import { userVisibleErrorMessage } from "@/shared/i18n/apiErrorMessage";
import { Button } from "@/shared/ui/Button";
import { TaloDesktopDialog } from "./TaloDesktopDialog";
import {
  taloHasConversation,
  taloSetupSteps,
  useManagedWorkerStatus,
  useTaloController,
} from "./talo-lifecycle";

export { taloHasConversation, useManagedWorkerStatus };

export function TaloPanel() {
  const canUseTalo = useCanUseTalo();
  if (!canUseTalo) return null;
  return <AuthorizedTaloSetupPanel />;
}

function AuthorizedTaloSetupPanel() {
  const { t } = useTranslation(["computers", "common"]);
  const [desktopNodeId, setDesktopNodeId] = useState<string>();
  const primaryRef = useRef<HTMLButtonElement>(null);
  const { status, poll, instance, phase, operation, transitioning, busy, error, perform } =
    useTaloController(undefined, (nodeId) => setDesktopNodeId(nodeId));

  useEffect(() => {
    if (desktopNodeId && instance?.nodeId !== desktopNodeId) {
      setDesktopNodeId(undefined);
    }
  }, [desktopNodeId, instance?.nodeId]);

  if (status.isPending) return <p role="status" className="py-6 text-sm text-muted">{t("common:loading")}</p>;
  if (status.error) {
    return (
      <div className="py-6">
        <p role="alert" className="text-sm text-danger">{userVisibleErrorMessage(status.error, t("computers:talo.loadError"))}</p>
        <Button type="button" variant="ghost" className="mt-2" onClick={() => void status.refetch()}>{t("common:retryNow")}</Button>
      </div>
    );
  }
  if (!status.data.enabled) return <p className="py-6 text-sm text-muted">{t("computers:talo.unavailable")}</p>;

  const stepIndex = taloSetupSteps.findIndex((step) => step === phase);
  const pollExpired = transitioning && poll.current !== null && Date.now() >= poll.current.until;
  const help = stepIndex !== -1 ? "pendingHelp" : phase === "ready" ? "readyHelp"
    : phase === "sleeping" ? "sleepingHelp" : phase === "stopped" ? "stoppedHelp"
      : phase === "offline" ? "offlineHelp" : phase ? null : "initializeHelp";
  const title = stepIndex !== -1 ? "initializing" : phase ? `phase.${phase}` as const : "initialize";
  const failedDelete = phase === "failed" && operation === "delete";
  const primary = !instance ? { label: "initialize", run: () => perform("create") }
    : phase === "ready" || phase === "sleeping" ? { label: "connect", run: () => perform("connect") }
    : phase === "failed" && operation === "stop" ? { label: "retryStop", run: () => perform("stop") }
    : failedDelete ? null
    : phase === "stopped" ? { label: "start", run: () => perform("start") }
    : phase === "offline" || phase === "failed" ? { label: "retryStart", run: () => perform("start") } : null;

  return (
    <>
      <div className="flex items-start gap-3 py-6">
        <span className="flex size-11 shrink-0 items-center justify-center rounded-xl border border-border bg-surface text-accent">
          <Bot size={22} aria-hidden="true" />
        </span>
        <div className="min-w-0" role="status">
          <h2 className="text-base font-semibold text-ink">{t(`computers:talo.${title}`)}</h2>
          {help ? <p className="mt-1 text-sm leading-6 text-muted">{t(`computers:talo.${help}`)}</p> : null}
        </div>
      </div>

      {stepIndex !== -1 ? (
        <ol className="divide-y divide-border rounded-xl border border-border bg-surface px-3">
          {taloSetupSteps.map((step, index) => (
            <li
              key={step}
              aria-current={index === stepIndex ? "step" : undefined}
              className="flex min-h-10 items-center gap-2.5 text-sm text-muted aria-[current=step]:font-medium aria-[current=step]:text-ink"
            >
              {index < stepIndex ? <Check size={15} aria-hidden="true" className="text-success" />
                : index === stepIndex ? <Loader2 size={15} aria-hidden="true" className="animate-spin text-accent motion-reduce:animate-none" />
                : <Circle size={15} aria-hidden="true" />}
              {t(`computers:talo.phase.${step}`)}
            </li>
          ))}
        </ol>
      ) : null}

      {error ? (
        <p role="alert" className="mb-3 rounded-lg border border-danger/20 bg-danger/5 px-3 py-2 text-sm text-danger">
          {error}
        </p>
      ) : null}

      <div className="mt-3 flex flex-wrap items-start justify-between gap-3 border-t border-border pt-4">
        {!instance ? <p className="my-auto text-xs text-muted">{t("computers:talo.accountLimit")}</p> : null}
        <div className="ml-auto flex flex-col items-end gap-2">
          {pollExpired ? (
            <Button
              type="button"
              variant="ghost"
              onClick={() => {
                poll.current = null;
                void status.refetch();
              }}
            >
              {t("computers:talo.refresh")}
            </Button>
          ) : null}
          {primary || busy ? (
            <Button ref={primaryRef} type="button" disabled={busy} onClick={primary?.run}>
              {busy ? t("computers:talo.working") : primary ? t(`computers:talo.${primary.label}`) : null}
            </Button>
          ) : null}
          {phase === "ready" && instance?.diskQuota === "unsupported" ? (
            <p className="max-w-56 text-right text-xs leading-5 text-muted">{t("computers:talo.quotaUnsupported")}</p>
          ) : null}
        </div>
      </div>

      <TaloDesktopDialog
        nodeId={desktopNodeId}
        open={Boolean(desktopNodeId && instance?.nodeId === desktopNodeId)}
        returnFocusRef={primaryRef}
        onOpenChange={(open) => { if (!open) setDesktopNodeId(undefined); }}
      />
    </>
  );
}
