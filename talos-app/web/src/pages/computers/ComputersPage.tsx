import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router-dom";
import { GitBranch, List, Plus, Search } from "lucide-react";
import {
  ComputerGraph,
  ComputerConnectHelp,
  ComputerTopologyList,
  ComputerInspector,
  ComputerServiceDetails,
  SyncControl,
  useAccountSyncMetrics,
  useSyncStatus,
  useComputerList,
  useComputers,
  observationsDelayed,
  type ComputerService,
  type FlowLayer,
} from "@/features/computers";
import { useAppStore } from "@/shared/store/app-store";
import {
  apiErrorMessage,
  userVisibleErrorMessage,
} from "@/shared/i18n/apiErrorMessage";
import { Button } from "@/shared/ui/Button";
import "./computers.css";

export function ComputersPage() {
  const { t, i18n } = useTranslation(["computers", "common"]);
  const computers = useComputers();
  const navigate = useNavigate();
  const [search, setSearch] = useState("");
  const [view, setView] = useState<"topology" | "list">("topology");
  const [layer, setLayer] = useState<FlowLayer>("all");
  const [help, setHelp] = useState(false);
  const [inspection, setInspection] = useState<{
    scope: string;
    nodeId?: string;
    service?: ComputerService;
  }>();
  const list = useComputerList(search.trim());
  const metrics = useAccountSyncMetrics();
  const status = useSyncStatus();
  const syncStatus = status.error ? undefined : status.data;
  const connection = useAppStore((state) => state.connection);
  const controlPlane =
    connection.controlPlaneEndpoint || window.location.origin;
  const nodes = list.data?.pages.flatMap((page) => page.data) ?? [];
  const current =
    inspection?.scope === computers.scope ? inspection : undefined;
  const inspectedId = current?.service
    ? undefined
    : (current?.nodeId ??
      nodes.find((node) => node.nodeId === computers.localId)?.nodeId ??
      nodes[0]?.nodeId);
  const service =
    current?.service ?? (!inspectedId ? "controlPlane" : undefined);
  const open = (id: string) => {
    computers.select(id);
    navigate("/");
  };
  const inspect = (id: string) =>
    setInspection({ scope: computers.scope, nodeId: id });
  const inspectService = (value: ComputerService) =>
    setInspection({ scope: computers.scope, service: value });
  const accountTotal = computers.list.data && !computers.list.error
    ? computers.total
    : undefined;
  const matchedTotal = list.data && !list.error
    ? list.data.pages[0]?.total
    : undefined;
  const loaded = matchedTotal === undefined ? "—" : t("computers:loadedComputers", {
    loaded: nodes.length,
    total: matchedTotal,
    count: matchedTotal,
  });
  const matches = search.trim() && matchedTotal !== undefined && accountTotal !== undefined
    ? t("computers:matchedComputers", { matched: matchedTotal, total: accountTotal })
    : undefined;
  return (
    <div className="cmp-page">
      <div className="cmp-breadcrumb">
        Talos <span>/</span> {t("computers:title")}
      </div>
      <header className="cmp-page-heading">
        <div>
          <h1>
            {t("computers:title")}
            <span>.</span>
          </h1>
          <p>{t("computers:managementSubtitle")}</p>
        </div>
        <Button
          onClick={() => setHelp(!help)}
          aria-expanded={help}
          aria-controls="computer-connect-help"
        >
          <Plus size={16} aria-hidden="true" />
          {t("computers:add")}
        </Button>
      </header>
      {help ? <ComputerConnectHelp scope={computers.scope} /> : null}
      <section className="cmp-overview" aria-label={t("computers:title")}>
        <span>
          <strong>
            {accountTotal?.toLocaleString(i18n.language) ?? "—"}
          </strong>{" "}
          {t("computers:title")}
        </span>
        <span>
          <i className="cmp-dot is-online" />
          <strong>
            {list.data && !list.error
              ? nodes.filter((node) => node.available).length
              : "—"}
          </strong>
          {t("computers:onlineInView")}
        </span>
        <span className="cmp-sync-text">
          <strong>
            {metrics.data?.peers.length.toLocaleString(i18n.language) ?? "—"}
          </strong>
          {t("computers:peerCount", { count: metrics.data?.peers.length ?? 0 })}
        </span>
      </section>
      <div className="cmp-controls">
        <SyncControl metrics={metrics.data} metricsError={metrics.error} />
      </div>
      {computers.runtime?.error ? (
        <p role="alert" className="cmp-notice text-danger">
          {apiErrorMessage(computers.runtime.error)}
        </p>
      ) : null}
      {!metrics.enabled ? (
        <p className="cmp-notice">{t("computers:metricsSignIn")}</p>
      ) : metrics.isPending ? (
        <p role="status" className="cmp-notice">
          {t("computers:metricsLoading")}
        </p>
      ) : metrics.data && observationsDelayed(metrics.data) ? (
        <p role="status" className="cmp-notice">
          {t("computers:metricsIncomplete")}
        </p>
      ) : null}
      <div className="cmp-tools">
        <div className="cmp-search-area">
          <label className="cmp-search">
            <Search size={16} aria-hidden="true" />
            <input
              aria-label={t("computers:search")}
              placeholder={t("computers:search")}
              value={search}
              onChange={(event) => setSearch(event.target.value)}
            />
          </label>
          {matches ? <span className="cmp-search-results" role="status">{matches}</span> : null}
        </div>
        <div className="cmp-segments" aria-label={t("computers:viewMode")}>
          <button
            type="button"
            aria-pressed={view === "topology"}
            onClick={() => setView("topology")}
          >
            <GitBranch size={14} aria-hidden="true" />
            {t("computers:topologyView")}
          </button>
          <button
            type="button"
            aria-pressed={view === "list"}
            onClick={() => setView("list")}
          >
            <List size={14} aria-hidden="true" />
            {t("computers:listView")}
          </button>
        </div>
      </div>
      <div className="cmp-content">
        <section className="cmp-panel">
          <div className="cmp-panel-heading">
            <div>
              <h2>
                {t(
                  view === "topology"
                    ? "computers:topologyTitle"
                    : "computers:title",
                )}
              </h2>
            </div>
            {view === "topology" ? (
              <div
                className="cmp-flow-buttons"
                aria-label={t("computers:flowFilter")}
              >
                {(["all", "execution", "sync"] as const).map((value) => (
                  <button
                    type="button"
                    key={value}
                    aria-pressed={layer === value}
                    onClick={() => setLayer(value)}
                  >
                    {value !== "all" ? (
                      <i
                        className={`cmp-flow-key ${value === "sync" ? "is-sync" : ""}`}
                      />
                    ) : null}
                    {t(
                      value === "all"
                        ? "computers:allFlows"
                        : value === "execution"
                          ? "computers:executionFlow"
                          : "computers:syncFlow",
                    )}
                  </button>
                ))}
              </div>
            ) : null}
          </div>
          {view === "topology" ? (
            <p className="cmp-mobile-hint">{t("computers:graphScroll")}</p>
          ) : null}
          {list.error ? (
            <p role="alert" className="cmp-notice text-danger">
              {userVisibleErrorMessage(
                list.error,
                t("computers:directoryError"),
              )}
            </p>
          ) : list.isPending ? (
            <p role="status" className="cmp-empty">
              {t("common:loading")}
            </p>
          ) : view === "topology" ? (
            <ComputerGraph
              nodes={nodes}
              metrics={metrics.data}
              syncEndpoint={metrics.endpoint}
              syncStatus={syncStatus}
              selectedId={inspectedId}
              selectedService={service}
              layer={layer}
              controlPlane={controlPlane}
              onInspect={inspect}
              onService={inspectService}
            />
          ) : (
            <ComputerTopologyList
              nodes={nodes}
              metrics={metrics.data}
              syncStatus={syncStatus}
              selectedId={inspectedId}
              onInspect={inspect}
            />
          )}
          {list.hasNextPage ? (
            <button
              type="button"
              className="cmp-load-more"
              disabled={list.isFetchingNextPage}
              onClick={() => void list.fetchNextPage()}
            >
              {t("common:showMore")} · {loaded}
            </button>
          ) : null}
          <footer className="cmp-canvas-footer">
            <span>{t("computers:inspectHint")}</span>
            <details>
              <summary>{t("computers:flowHelp")}</summary>
              <p>{t("computers:flowHelpText")}</p>
            </details>
          </footer>
        </section>
        <aside
          className="cmp-panel cmp-inspector"
          aria-label={t("computers:manage")}
        >
          {service ? (
            <ComputerServiceDetails
              service={service}
              metrics={metrics.data}
              syncEndpoint={metrics.endpoint}
              syncStatus={syncStatus}
              controlPlane={controlPlane}
            />
          ) : inspectedId ? (
            <ComputerInspector
              key={`${computers.scope}:${inspectedId}`}
              nodeId={inspectedId}
              localId={computers.localId}
              controlPlane={controlPlane}
              metrics={metrics.data}
              syncStatus={syncStatus}
              onOpen={open}
              onDeleted={() => inspectService("controlPlane")}
            />
          ) : null}
        </aside>
      </div>
    </div>
  );
}
