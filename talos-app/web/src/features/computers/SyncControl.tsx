import { AppProtocol } from "@talos/sdk-core";
import { Link } from "react-router-dom";
import { useState } from "react";
import { Cloud, CloudOff } from "lucide-react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { getTalosClient } from "@/shared/api/client";
import { useAppStore } from "@/shared/store/app-store";
import { apiErrorMessage, userVisibleError } from "@/shared/i18n/apiErrorMessage";
import { syncObservationState, SyncTopologyState, syncStateLabel } from "./topology/sync-state";
import { useProductScope, useSyncStatus } from "./hooks";

/** Sharing is independent of the selected Computer and never disables local execution. */
export function SyncControl({ metrics, metricsError }: { metrics?: AppProtocol.SyncMetricsResponse; metricsError?: unknown }) {
  const { t } = useTranslation("computers");
  const scope = useProductScope();
  const status = useSyncStatus();
  const queries = useQueryClient();
  const auth = useAppStore((state) => state.connection.authStatus);
  const [claim, setClaim] = useState(false);
  const mutation = useMutation({ mutationFn: (enabled: boolean) => enabled
    ? getTalosClient().sync.enable({ claimLocalHistory: claim }) : getTalosClient().sync.disable(),
    onSuccess: (data) => { queries.setQueryData(["talos", scope, "sync"], data); void queries.invalidateQueries({ queryKey: ["talos", scope] }); } });
  const state = status.data?.connection ?? (auth === "authenticated" && status.isPending ? "connecting" : "disabled");
  const peers = metrics?.peers.filter((peer) => peer.peerId === status.data?.peerId) ?? [];
  const observed = status.error ? SyncTopologyState.Unknown : syncObservationState(metrics, peers, status.data);
  const Icon = state === "disabled" ? CloudOff : Cloud;
  const label = t(syncStateLabel[observed]);
  const enabled = state !== "disabled" || status.data?.configuredEnabled === true;
  // This control owns Sync failures for the page, including telemetry reads.
  // Preserve specific errors; localize the known transport failure by code.
  const requestError = mutation.error ?? status.error;
  const error = requestError ? userVisibleError(requestError, t("syncError"))
    : status.data?.lastError ?? (metricsError ? userVisibleError(metricsError, t("metricsUnavailable")) : undefined);
  const unavailable = !mutation.error && (error?.code === AppProtocol.ErrorCode.ServiceUnavailable || error?.code === AppProtocol.ErrorCode.Timeout);
  const errorMessage = error && (unavailable
    ? t(state === "disconnected" || state === "connecting" ? "syncUnavailable" : "metricsUnavailable")
    : apiErrorMessage(error));
  return <section className="mb-6 rounded-xl border border-border bg-card px-4 py-3" aria-label={t("syncTitle")} data-testid="sync-control">
    <div className="flex items-center justify-between gap-4"><div className="flex items-center gap-2"><Icon size={16} className={observed === SyncTopologyState.Conflict || observed === SyncTopologyState.Error ? "text-danger" : state === "connected" ? "text-success" : "text-muted"} /><div><p className="text-sm font-medium">{t("syncTitle")}</p><p className="text-xs text-muted" role="status">{label}{status.data?.pendingChanges ? ` · ${t("syncPending", { count: status.data.pendingChanges })}` : ""}</p></div></div>
      {auth === "authenticated" ? <button type="button" role="switch" aria-checked={enabled} aria-label={t("syncTitle")} disabled={mutation.isPending || status.isPending} onClick={() => mutation.mutate(!enabled)} className={`relative h-5 w-9 shrink-0 rounded-full transition disabled:opacity-50 ${enabled ? "bg-brand" : "bg-muted/30"}`}><span className={`absolute top-0.5 size-4 rounded-full bg-background transition ${enabled ? "left-[18px]" : "left-0.5"}`} /></button> : <Link className="text-xs text-brand" to="/auth/login?next=%2Fcomputers">{t("syncSignIn")}</Link>}
    </div>
    <p className="mt-2 text-xs text-muted">{t("syncHelp")}</p>
    {auth === "authenticated" && !enabled && status.data?.canClaimLocalHistory ? <label className="mt-3 flex items-start gap-2 text-xs text-muted"><input type="checkbox" checked={claim} onChange={(event) => setClaim(event.target.checked)} />{t("syncClaim")}</label> : null}
    {errorMessage ? <p role="alert" className="mt-2 text-xs text-danger">{errorMessage}</p> : null}
  </section>;
}
