import { formatDuration } from "@/shared/lib/formatDuration";
import { useTranslation } from "react-i18next";
import { observationsFresh, syncPeerConnected } from "./sync-state";
import { SyncProgressSummary } from "./SyncProgressSummary";
import type { AppProtocol } from "@talos/sdk-core";

export function SyncPeerDetails({
  peers,
  metrics,
  syncStatus,
}: {
  peers: AppProtocol.SyncPeerMetrics[];
  metrics?: AppProtocol.SyncMetricsResponse;
  syncStatus?: AppProtocol.SyncStatusResponse;
}) {
  const { t, i18n } = useTranslation("computers");
  const number = (value: number) =>
    value.toLocaleString(i18n.language, { maximumFractionDigits: 1 });
  const fresh = observationsFresh(metrics);
  const rows: (AppProtocol.SyncPeerMetrics | undefined)[] = peers.length ? peers : [undefined];
  return (
    <section className="cmp-peer-details" aria-label={t("syncTitle")}
      title={!fresh && metrics ? t("observationsExpired") : undefined}>
      <h3>{t("syncTitle")}</h3>
      <SyncProgressSummary metrics={metrics} peers={peers} syncStatus={syncStatus} showConfirmation />
      {rows.map((peer) => {
        const live = fresh && peer !== undefined && syncPeerConnected(peer, syncStatus);
        const heartbeatAt = peer?.lastHeartbeatAt;
        const activityAt = fresh ? peer?.lastActivityAt : undefined;
        return <div className="cmp-peer" key={peer?.peerId ?? "unobserved"}>
          {peers.length > 1 ? <SyncProgressSummary metrics={metrics} peers={peer ? [peer] : []}
            syncStatus={peer?.peerId === syncStatus?.peerId ? syncStatus : undefined} showConfirmation /> : null}
          <dl>
            <div><dt>{t("peerIdentity")}</dt><dd className="cmp-mono">{peer?.peerId ?? syncStatus?.peerId ?? "N/A"}</dd></div>
            <div><dt>{t("connectionCount")}</dt><dd>{fresh && peer && (peer.connections === 0 || live) ? t("connectionsWithCount", { count: peer.connections }) : "N/A"}</dd></div>
            <div><dt>{t("inboundRate")}</dt><dd>{live ? `${number(peer.inboundBytesPerSecond)} B/s` : "N/A"}</dd></div>
            <div><dt>{t("outboundRate")}</dt><dd>{live ? `${number(peer.outboundBytesPerSecond)} B/s` : "N/A"}</dd></div>
            <div><dt>{t("updateRate")}</dt><dd>{live ? number(peer.updatesPerSecond) : "N/A"}</dd></div>
            <div><dt>{t("commitP95")}</dt><dd>{live && peer.commitP95Ms !== null ? formatDuration(peer.commitP95Ms, i18n.language) : "N/A"}</dd></div>
            <div><dt>{t("activeRooms")}</dt><dd>{live ? peer.documentRooms : "N/A"}</dd></div>
            <div>
              <dt>{t("lastSyncHeartbeat")}</dt>
              <dd>{heartbeatAt === undefined || heartbeatAt === null ? "N/A" :
                <time dateTime={new Date(heartbeatAt * 1000).toISOString()}
                  title={new Date(heartbeatAt * 1000).toLocaleString(i18n.language)}>
                  {t("syncHeartbeatAgo", { duration: formatDuration(Math.max(0, Date.now() - heartbeatAt * 1000), i18n.language) })}
                </time>}</dd>
            </div>
            {peer?.progress?.sourceDelays.map((sample) => <div key={sample.sourcePeerId}>
              <dt title={sample.sourcePeerId}>{t("fromPeer", { peer: sample.sourcePeerId.slice(0, 8) })}</dt>
              <dd>{live ? formatDuration(sample.observedDelayMs, i18n.language) : "N/A"}</dd>
            </div>)}
            <div><dt>{t("lastActivity")}</dt><dd>{activityAt === undefined || activityAt === null
              ? "N/A" : new Date(activityAt * 1000).toLocaleTimeString(i18n.language)}</dd></div>
          </dl>
        </div>;
      })}
      {fresh && peers.some((peer) => syncPeerConnected(peer, syncStatus) && peer.progress?.sourceDelays.length) ?
        <p className="cmp-footnote">{t("sourceDelayHelp")}</p> : null}
      <p className="cmp-footnote">{t("selfReported")}</p>
    </section>
  );
}
