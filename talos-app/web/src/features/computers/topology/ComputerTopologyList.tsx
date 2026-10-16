import { useTranslation } from "react-i18next";
import type { AppProtocol } from "@talos/sdk-core";
import { syncStateLabel } from "./sync-state";
import { computerPeers, computerPlatform, computerSyncState } from "./observations";

interface Props {
  nodes: AppProtocol.NodeInfo[];
  metrics?: AppProtocol.SyncMetricsResponse;
  syncStatus?: AppProtocol.SyncStatusResponse;
  selectedId?: string;
  onInspect: (id: string) => void;
}

export function ComputerTopologyList({
  nodes,
  metrics,
  syncStatus,
  selectedId,
  onInspect,
}: Props) {
  const { t, i18n } = useTranslation("computers");
  return (
    <div className="cmp-table-scroll">
      <table className="cmp-table">
        <thead>
          <tr>
            <th>Computer</th>
            <th>{t("system")}</th>
            <th>{t("executionFlow")}</th>
            <th>{t("syncTitle")}</th>
            <th>{t("lastActivity")}</th>
          </tr>
        </thead>
        <tbody>
          {nodes.map((node) => {
            const peers = computerPeers(metrics, node.nodeId);
            const state = computerSyncState(metrics, node.nodeId, syncStatus);
            const activity = peers.flatMap((peer) =>
              peer.lastActivityAt === null ? [] : [peer.lastActivityAt],
            );
            const last = activity.length ? Math.max(...activity) : null;
            return (
              <tr
                key={node.nodeId}
                className={selectedId === node.nodeId ? "is-selected" : ""}
              >
                <td>
                  <button
                    type="button"
                    className="cmp-name-button"
                    onClick={() => onInspect(node.nodeId)}
                    aria-pressed={selectedId === node.nodeId}
                  >
                    {node.displayName}
                  </button>
                  <small className="cmp-mono">{node.nodeId}</small>
                </td>
                <td>
                  {computerPlatform(node) ?? t("unknown")}
                </td>
                <td>
                  <span className="cmp-inline-status">
                    <i
                      className={`cmp-dot ${node.available ? "is-online" : ""}`}
                    />
                    {t(
                      node.status === "disabled"
                        ? "remoteDisabled"
                        : node.available
                          ? "connected"
                          : "offline",
                    )}
                  </span>
                </td>
                <td data-sync-state={state} className={`cmp-sync-status is-${state}`}>
                  {t(syncStateLabel[state])}
                </td>
                <td>
                  {last === null
                    ? "—"
                    : new Date(last * 1000).toLocaleTimeString(i18n.language)}
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
      {nodes.length === 0 ? (
        <p className="cmp-empty">{t("graphEmpty")}</p>
      ) : null}
    </div>
  );
}
