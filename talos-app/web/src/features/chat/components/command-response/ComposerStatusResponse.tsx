import { Gauge } from "lucide-react";
import { useTranslation } from "react-i18next";

import {
  contextRemainingTokens,
  contextUsedPercent,
  formatTokenCount,
  type ContextUsage,
} from "../contextUsage";
import { CommandResponseSurface } from "./CommandResponseSurface";
import type { ComposerStatusSnapshot } from "./command-response.types";

interface ComposerStatusResponseProps {
  contextUsage?: ContextUsage;
  onDismiss?: () => void;
  sessionTotalTokens?: number;
  status: ComposerStatusSnapshot;
}

/** Renders a live Web status snapshot using the same domain semantics as the TUI status card. */
export function ComposerStatusResponse({
  contextUsage,
  onDismiss,
  sessionTotalTokens,
  status,
}: ComposerStatusResponseProps) {
  const { t } = useTranslation(["chat", "common"]);
  const usedPercent = contextUsage ? contextUsedPercent(contextUsage) : undefined;
  const stateLabel = status.state === "running"
    ? t("chat:sessionStatusRunning")
    : t("chat:sessionStatusReady");

  return (
    <CommandResponseSurface
      actions={(
        <span className="flex items-center gap-1.5 text-[11px] font-medium text-muted">
          <span
            className={status.state === "running"
              ? "size-1.5 animate-pulse rounded-full bg-brand"
              : "size-1.5 rounded-full bg-success"}
          />
          {stateLabel}
        </span>
      )}
      icon={<Gauge aria-hidden="true" size={14} />}
      label={t("chat:commandStatusRegionLabel")}
      onDismiss={onDismiss}
      role="region"
      title={t("chat:commandStatusTitle")}
    >
      <div className="grid gap-x-7 gap-y-3 px-4 py-3.5 sm:grid-cols-2">
        <StatusMetric label={t("chat:modelLabel")} value={formatModel(status, t)} />
        <StatusMetric label={t("chat:commandStatusProject")} value={status.project ?? t("chat:commandStatusNoProject")} />
        <StatusMetric label={t("chat:commandStatusSession")} mono value={abbreviateSessionId(status.sessionId)} />

        {contextUsage && usedPercent !== undefined ? (
          <div className="sm:col-span-2">
            <div className="mb-1.5 flex items-end justify-between gap-4">
              <span className="text-[10px] font-medium uppercase tracking-[0.1em] text-muted">
                {t("chat:commandStatusContext")}
              </span>
              <span className="text-[11px] tabular-nums text-muted">
                {t("chat:commandStatusContextRatio", {
                  total: formatTokenCount(contextUsage.contextWindowTokens),
                  used: formatTokenCount(contextUsage.usedTokens),
                })}
              </span>
            </div>
            <div
              aria-label={t("chat:commandStatusContextUsage", { percent: usedPercent.toFixed(1) })}
              aria-valuemax={100}
              aria-valuemin={0}
              aria-valuenow={usedPercent}
              className="h-1.5 overflow-hidden rounded-full bg-surface"
              role="progressbar"
            >
              <div className="h-full rounded-full bg-brand transition-[width] duration-300" style={{ width: `${usedPercent}%` }} />
            </div>
            <div className="mt-1.5 flex flex-wrap items-center justify-between gap-x-4 gap-y-1 text-[11px] tabular-nums text-muted">
              <span>{t("chat:commandStatusContextUsed", { percent: usedPercent.toFixed(1) })}</span>
              <span>
                {t("chat:commandStatusContextRemaining", {
                  value: formatTokenCount(contextRemainingTokens(contextUsage)),
                })}
              </span>
            </div>
          </div>
        ) : null}

        {sessionTotalTokens !== undefined ? (
          <div className="sm:col-span-2 text-[11px] text-muted">
            {t("chat:commandStatusSessionTotal", { value: formatTokenCount(sessionTotalTokens) })}
          </div>
        ) : null}
      </div>
    </CommandResponseSurface>
  );
}

function StatusMetric({ label, mono = false, value }: { label: string; mono?: boolean; value: string }) {
  return (
    <div className="min-w-0">
      <div className="mb-0.5 text-[10px] font-medium uppercase tracking-[0.1em] text-muted">{label}</div>
      <div className={mono
        ? "truncate font-mono text-[11px] text-ink/85"
        : "truncate text-[12px] font-medium text-ink/90"}
      >
        {value}
      </div>
    </div>
  );
}

function formatModel(
  status: ComposerStatusSnapshot,
  t: ReturnType<typeof useTranslation>["t"],
) {
  if (!status.reasoningLevel || status.reasoningLevel === "none") {
    return status.model;
  }
  const reasoning = t(`chat:reasoning${capitalize(status.reasoningLevel)}`);
  return `${status.model} · ${reasoning}`;
}

function abbreviateSessionId(sessionId: string) {
  return sessionId.length > 22 ? `${sessionId.slice(0, 8)}…${sessionId.slice(-6)}` : sessionId;
}

function capitalize(value: string) {
  return `${value.charAt(0).toUpperCase()}${value.slice(1)}`.replace("_high", "High");
}
