import { formatDuration } from "@/shared/lib/formatDuration";
import { useTranslation } from "react-i18next";
import type { AppProtocol } from "@talos/sdk-core";
import { syncObservationState, syncProgressSummary, syncStateLabel } from "./sync-state";

/** Shared connection and durable progress facts for graph, list and inspector. */
export function SyncProgressSummary({
  metrics,
  peers,
  syncStatus,
  showConfirmation = false,
}: {
  metrics?: AppProtocol.SyncMetricsResponse;
  peers: AppProtocol.SyncPeerMetrics[];
  syncStatus?: AppProtocol.SyncStatusResponse;
  showConfirmation?: boolean;
}) {
  const { t, i18n } = useTranslation("computers");
  const state = syncObservationState(metrics, peers, syncStatus);
  const progress = syncProgressSummary(metrics, peers, syncStatus);
  const number = (value: number) => value.toLocaleString(i18n.language, { maximumFractionDigits: 1 });
  const download = progress.state === "caughtUp" ? `${t("syncCaughtUp")} · ${formatDuration(0, i18n.language)}`
    : progress.state === "recovering" ? [t("syncRecovering"), progress.downloadLagMs === null
      ? null : t("syncLagDuration", { duration: formatDuration(progress.downloadLagMs, i18n.language) })].filter(Boolean).join(" · ")
      : progress.state === "behind" ? progress.downloadLagMs === null
        ? t("syncBehind") : t("syncLagDuration", { duration: formatDuration(progress.downloadLagMs, i18n.language) })
        : "N/A";
  return <span className="cmp-sync-summary">
    <span data-sync-state={state} className={`cmp-sync-status is-${state}`}>{t(syncStateLabel[state])}</span>
    <span className="cmp-sync-progress" data-download-state={progress.state}>
      <span>{t("replicationProgress")}: {download}</span>
    <span>{t("pendingUploads")}: {progress.pendingUploads === null ? "N/A" : number(progress.pendingUploads)}</span>
    </span>
    {showConfirmation ? <span className="cmp-muted">
      {t("progressConfirmedAt")}: {progress.observedAt === null ? "N/A" : new Date(progress.observedAt * 1000).toLocaleTimeString(i18n.language)}
    </span> : null}
  </span>;
}
