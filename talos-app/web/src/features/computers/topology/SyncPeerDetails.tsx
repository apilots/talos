import { useTranslation } from "react-i18next";
import type { AppProtocol } from "@talos/sdk-core";

export function SyncPeerDetails({
  peers,
}: {
  peers: AppProtocol.SyncPeerMetrics[];
}) {
  const { t, i18n } = useTranslation("computers");
  const number = (value: number) =>
    value.toLocaleString(i18n.language, { maximumFractionDigits: 1 });
  return (
    <section className="cmp-peer-details" aria-label={t("syncTitle")}>
      <h3>{t("syncTitle")}</h3>
      {peers.length === 0 ? (
        <p className="cmp-muted">{t("noPeerDetails")}</p>
      ) : (
        peers.map((peer) => (
          <div className="cmp-peer" key={peer.peerId}>
            <p className="cmp-mono" title={peer.peerId}>
              {peer.peerId}
            </p>
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
      <p className="cmp-footnote">{t("selfReported")}</p>
    </section>
  );
}
