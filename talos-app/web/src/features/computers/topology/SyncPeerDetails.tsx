import { useTranslation } from "react-i18next";
import { observationsFresh } from "./sync-state";
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
  const currentPeers = fresh ? peers : [];
  return (
    <section className="cmp-peer-details" aria-label={t("syncTitle")}>
      <h3>{t("syncTitle")}</h3>
      <SyncProgressSummary metrics={metrics} peers={peers} syncStatus={syncStatus} showConfirmation />
      {!fresh && metrics ? <p className="cmp-footnote">{t("observationsExpired")}</p> : null}
      {currentPeers.length === 0 ? (
        <p className="cmp-muted">{t("noPeerDetails")}</p>
      ) : (
        currentPeers.map((peer) => (
          <div className="cmp-peer" key={peer.peerId}>
            <p className="cmp-mono" title={peer.peerId}>
              {peer.peerId}
            </p>
            {currentPeers.length > 1 ? <SyncProgressSummary metrics={metrics} peers={[peer]}
              syncStatus={peer.peerId === syncStatus?.peerId ? syncStatus : undefined} showConfirmation /> : null}
            <dl>
              <div>
                <dt>{t("connectionCount")}</dt>
                <dd>
                  {t("connectionsWithCount", { count: peer.connections })}
                </dd>
              </div>
              <div>
                <dt>{t("inboundRate")}</dt>
                <dd>{number(peer.inboundBytesPerSecond)} B/s</dd>
              </div>
              <div>
                <dt>{t("outboundRate")}</dt>
                <dd>{number(peer.outboundBytesPerSecond)} B/s</dd>
              </div>
              <div>
                <dt>{t("updateRate")}</dt>
                <dd>{number(peer.updatesPerSecond)}</dd>
              </div>
              <div>
                <dt>{t("commitP95")}</dt>
                <dd>
                  {peer.commitP95Ms === null
                    ? "—"
                    : `${number(peer.commitP95Ms)} ms`}
                </dd>
              </div>
              <div>
                <dt>{t("activeRooms")}</dt>
                <dd>{peer.documentRooms}</dd>
              </div>
              <div>
                <dt>{t("lastSyncHeartbeat")}</dt>
                <dd>{peer.lastHeartbeatAt === null ? t("noHeartbeat") :
                  new Date(peer.lastHeartbeatAt * 1000).toLocaleTimeString(i18n.language)}</dd>
              </div>
              {peer.progress?.sourceDelays.map((sample) => (
                <div key={sample.sourcePeerId}>
                  <dt title={sample.sourcePeerId}>{t("fromPeer", { peer: sample.sourcePeerId.slice(0, 8) })}</dt>
                  <dd>{number(sample.observedDelayMs)} ms</dd>
                </div>
              ))}
              <div>
                <dt>{t("lastActivity")}</dt>
                <dd>
                  {peer.lastActivityAt === null
                    ? t("noActivity")
                    : new Date(peer.lastActivityAt * 1000).toLocaleTimeString(
                        i18n.language,
                      )}
                </dd>
              </div>
            </dl>
          </div>
        ))
      )}
      {currentPeers.some((peer) => peer.progress?.sourceDelays.length) ?
        <p className="cmp-footnote">{t("sourceDelayHelp")}</p> : null}
      <p className="cmp-footnote">{t("selfReported")}</p>
    </section>
  );
}
