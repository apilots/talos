import { useId } from "react";
import { useTranslation } from "react-i18next";
import { ArrowLeftRight, Layers, Laptop, Server } from "lucide-react";
import type { AppProtocol } from "@talos/sdk-core";
import {
  computerPeers,
  computerSyncState,
  controlPlaneSyncState,
  computerPlatform,
  type ComputerService,
  type FlowLayer,
} from "./observations";
import { syncStateLabel } from "./sync-state";

export interface ComputerGraphProps {
  nodes: AppProtocol.NodeInfo[];
  metrics?: AppProtocol.SyncMetricsResponse;
  syncStatus?: AppProtocol.SyncStatusResponse;
  selectedId?: string;
  selectedService?: ComputerService;
  controlPlane: string;
  layer: FlowLayer;
  onInspect: (id: string) => void;
  onService: (service: ComputerService) => void;
}

export function ComputerGraph({
  nodes,
  metrics,
  syncStatus,
  selectedId,
  selectedService,
  controlPlane,
  layer,
  onInspect,
  onService,
}: ComputerGraphProps) {
  const { t } = useTranslation("computers");
  const marker = useId().replaceAll(":", "");
  const height = Math.max(510, nodes.length * 126 + 80);
  const planeState = controlPlaneSyncState(metrics, syncStatus);
  return (
    <div
      className="cmp-canvas-scroll"
      tabIndex={0}
      aria-label={t("graphScroll")}
    >
      <div className="cmp-world" style={{ height }}>
        <span className="cmp-column cmp-services-label">
          {t("infrastructure")}
        </span>
        <span className="cmp-column cmp-computers-label">
          {t("yourComputers")}
        </span>
        <svg
          className="cmp-wires"
          viewBox={`0 0 840 ${height}`}
          preserveAspectRatio="none"
          role="img"
          aria-label={t("graphLabel")}
        >
          <defs>
            {["execution", "sync"].map((flow) => (
              <marker
                key={flow}
                id={`${marker}-${flow}`}
                viewBox="0 0 10 10"
                refX="9"
                refY="5"
                markerWidth="5"
                markerHeight="5"
                orient="auto-start-reverse"
              >
                <path
                  d="M 0 0 L 10 5 L 0 10 z"
                  className={`cmp-arrow-${flow}`}
                />
              </marker>
            ))}
          </defs>
          {nodes.map((node, index) => {
            const y = 125 + index * 126;
            const peers = computerPeers(metrics, node.nodeId);
            const state = computerSyncState(metrics, node.nodeId, syncStatus);
            return (
              <g key={node.nodeId}>
                {layer !== "sync" ? (
                  <path
                    data-flow="execution"
                    data-computer-id={node.nodeId}
                    d={`M286,125 C375,125 375,${y - 12} 468,${y - 12}`}
                    className={`cmp-execution-wire ${node.available ? "" : "is-offline"}`}
                    markerStart={`url(#${marker}-execution)`}
                    markerEnd={`url(#${marker}-execution)`}
                  >
                    <title>{`${node.displayName} · ${t("executionFlow")}`}</title>
                  </path>
                ) : null}
                {layer !== "execution" && (peers.length > 0 || syncStatus?.localComputerId === node.nodeId) ? (
                  <path
                    data-flow="sync"
                    data-computer-id={node.nodeId}
                    d={`M286,365 C405,365 405,${y + 20} 468,${y + 20}`}
                    data-sync-state={state}
                    className={`cmp-sync-wire is-${state}`}
                    markerStart={`url(#${marker}-sync)`}
                    markerEnd={`url(#${marker}-sync)`}
                  >
                    <title>{`${node.displayName} · ${t(syncStateLabel[state])}`}</title>
                  </path>
                ) : null}
              </g>
            );
          })}
          {layer !== "execution" && metrics?.controlPlanePeerId ? (
            <path
              data-flow="sync"
              data-service="controlPlane"
              d="M160,184 L160,304"
              data-sync-state={planeState}
              className={`cmp-sync-wire is-${planeState}`}
              markerStart={`url(#${marker}-sync)`}
              markerEnd={`url(#${marker}-sync)`}
            >
              <title>{t(syncStateLabel[planeState])}</title>
            </path>
          ) : null}
        </svg>
        <button
          type="button"
          className={`cmp-service cmp-plane ${selectedService === "controlPlane" ? "is-selected" : ""}`}
          onClick={() => onService("controlPlane")}
          aria-label={t("inspectService", { name: "Talos Control Plane" })}
          aria-pressed={selectedService === "controlPlane"}
        >
          <span className="cmp-node-icon">
            <Layers size={21} />
          </span>
          <span>
            <strong>Talos Control Plane</strong>
            <small>{controlPlane}</small>
          </span>
          <footer>
            {t("controlPlaneFacts")}
            <span>{t("fixedService")}</span>
          </footer>
        </button>
        <div className="cmp-service-relation">
          {metrics?.controlPlanePeerId ? (
            <>
              <ArrowLeftRight size={15} />
              <span data-sync-state={planeState}>{t(syncStateLabel[planeState])}</span>
            </>
          ) : (
            <span>{t("controlPlaneSyncUnknown")}</span>
          )}
        </div>
        <button
          type="button"
          className={`cmp-service cmp-sync ${selectedService === "sync" ? "is-selected" : ""}`}
          onClick={() => onService("sync")}
          aria-label={t("inspectService", { name: "Talos Sync" })}
          aria-pressed={selectedService === "sync"}
        >
          <span className="cmp-node-icon is-sync">
            <ArrowLeftRight size={21} />
          </span>
          <span>
            <strong>Talos Sync</strong>
            <small>{metrics?.endpoint ?? t("unknown")}</small>
          </span>
          <footer>
            {t("syncFacts")}
            <span>{t("fixedService")}</span>
          </footer>
        </button>
        {nodes.map((node, index) => {
          const peers = computerPeers(metrics, node.nodeId);
          const state = computerSyncState(metrics, node.nodeId, syncStatus);
          const Icon = node.systemInfo?.os === "macOs" ? Laptop : Server;
          return (
            <button
              key={node.nodeId}
              type="button"
              style={{ top: 70 + index * 126 }}
              className={`cmp-device ${selectedId === node.nodeId ? "is-selected" : ""} ${layer === "sync" && peers.length === 0 ? "is-dimmed" : ""}`}
              onClick={() => onInspect(node.nodeId)}
              aria-label={t("inspectComputer", { name: node.displayName })}
              aria-pressed={selectedId === node.nodeId}
            >
              <span className="cmp-node-icon">
                <Icon size={21} />
              </span>
              <span className="cmp-device-copy">
                <strong>{node.displayName}</strong>
                <small>
                  {computerPlatform(node) ?? t("unknown")}
                </small>
              </span>
              <i className={`cmp-dot ${node.available ? "is-online" : ""}`} />
              <footer>
                <span>
                  {t(
                    node.status === "disabled"
                      ? "remoteDisabled"
                      : node.available
                        ? "connected"
                        : "offline",
                  )}
                </span>
                <span data-sync-state={state} className={`cmp-sync-status is-${state}`}>
                  {t(syncStateLabel[state])}
                </span>
              </footer>
            </button>
          );
        })}
        {nodes.length === 0 ? (
          <p className="cmp-graph-empty">{t("graphEmpty")}</p>
        ) : null}
      </div>
    </div>
  );
}
