import { useTranslation } from "react-i18next";
import type { AppProtocol } from "@talos/sdk-core";
import { controlPlanePeer, type ComputerService } from "./observations";
import { SyncPeerDetails } from "./SyncPeerDetails";

export function ComputerServiceDetails({
  service,
  metrics,
  controlPlane,
  syncStatus,
}: {
  service: ComputerService;
  metrics?: AppProtocol.SyncMetricsResponse;
  controlPlane: string;
  syncStatus?: AppProtocol.SyncStatusResponse;
}) {
  const { t, i18n } = useTranslation("computers");
  const peer = controlPlanePeer(metrics);
  return (
    <>
      <div className="cmp-inspector-heading">
        <span className="cmp-eyebrow">{t("fixedService")}</span>
        <h2>{service === "sync" ? "Talos Sync" : "Talos Control Plane"}</h2>
        <p className="cmp-muted">
          {t(service === "sync" ? "syncFacts" : "controlPlaneFacts")}
        </p>
      </div>
      <dl className="cmp-facts">
        <div>
          <dt>{t("endpoint")}</dt>
          <dd>
            {service === "sync"
              ? (metrics?.endpoint ?? t("unknown"))
              : controlPlane}
          </dd>
        </div>
      </dl>
      {metrics ? (
        <p className="cmp-footnote">
          {t("lastObserved", {
            time: new Date(metrics.observedAt * 1000).toLocaleTimeString(
              i18n.language,
            ),
          })}
        </p>
      ) : (
        <p className="cmp-footnote">{t("syncUnknown")}</p>
      )}
      {service === "controlPlane" ? (
        <SyncPeerDetails
          peers={peer ? [peer] : []}
          metrics={metrics}
          syncStatus={
            metrics?.controlPlanePeerId && metrics.controlPlanePeerId === syncStatus?.peerId
              ? syncStatus
              : undefined
          }
        />
      ) : (
        <>
          <h3 className="mb-3 mt-6 text-sm font-medium">
            {t("resourceCounts")}
          </h3>
          <dl className="cmp-facts">
            {(["computers", "projects", "sessions", "files"] as const).map(
              (key) => (
                <div key={key}>
                  <dt>{t(`sync${key[0].toUpperCase()}${key.slice(1)}`)}</dt>
                  <dd>
                    {metrics?.metadata[key].toLocaleString(i18n.language) ??
                      "—"}
                  </dd>
                </div>
              ),
            )}
          </dl>
          {metrics ? (
            <p className="cmp-footnote">
              {t("peersWithCount", { count: metrics.peers.length })}
            </p>
          ) : null}
          {metrics ? (
            <p className="cmp-footnote">
              {t("unmappedPeers", {
                count: metrics.peers.filter(
                  (value) => value.computerIds.length === 0,
                ).length,
              })}
            </p>
          ) : null}
        </>
      )}
      <p className="cmp-footnote">{t("flowHelpText")}</p>
    </>
  );
}
