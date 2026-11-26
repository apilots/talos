import { useEffect, useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { createElectronAppServerClient, type TalosRuntimeState } from "@talos/sdk-app-server/desktop";
import { computerStatus } from "../computer-status";
import type { AppProtocol } from "@talos/sdk-core";
import { getTalosClient } from "@/shared/api/client";
import { getDesktopBridge } from "@/shared/api/desktop-client";
import { useAppStore } from "@/shared/store/app-store";
import { userVisibleErrorMessage } from "@/shared/i18n/apiErrorMessage";
import { Button } from "@/shared/ui/Button";
import { useComputer, useProductScope } from "../hooks";
import { ComputerDirectoryError } from "../ComputerDirectoryError";
import { ComputerDetails } from "../ComputerDetails";
import { ComputerDesktopButton } from "../ComputerDesktop";
import { SyncPeerDetails } from "./SyncPeerDetails";
import { ComputerRegistrationActions } from "./ComputerRegistrationActions";
import { computerPeers, computerLocalSyncStatus } from "./observations";

interface Props {
  nodeId: string;
  localId?: string;
  localNode?: AppProtocol.NodeInfo;
  listedNode?: AppProtocol.NodeInfo;
  localRuntime?: TalosRuntimeState;
  controlPlane: string;
  metrics?: AppProtocol.SyncMetricsResponse;
  syncStatus?: AppProtocol.SyncStatusResponse;
  onOpen: (id: string) => void;
  onDeleted: () => void;
}

/** Inspection is page-local. Execution selection changes only through onOpen. */
export function ComputerInspector({
  nodeId,
  localId,
  localNode,
  listedNode,
  localRuntime,
  controlPlane,
  metrics,
  syncStatus,
  onOpen,
  onDeleted,
}: Props) {
  const { t } = useTranslation(["computers", "common"]);
  const query = useComputer(nodeId, listedNode);
  const scope = useProductScope();
  const queries = useQueryClient();
  const local = nodeId === localId && localNode?.nodeId === nodeId;
  const directoryNode = query.error || query.isPlaceholderData ? undefined : query.data;
  const node = local ? { ...localNode, ...directoryNode, available: localNode.available, systemInfo: localNode.systemInfo } : query.data;
  const connection = useAppStore((state) => state.connection);
  const [name, setName] = useState("");
  useEffect(() => {
    setName(node?.displayName ?? "");
  }, [node?.nodeId, node?.displayName]);
  const mutation = useMutation({
    mutationFn: async (action: "rename" | "status" | "recover") => {
      if (!node || !directoryNode) return;
      if (action === "recover") {
        const bridge = getDesktopBridge();
        if (bridge)
          await createElectronAppServerClient(bridge).desktop.enrollWorker({
            nodeId: localId ?? node.nodeId,
          });
      } else if (action === "rename") {
        await (await getTalosClient({ nodeId: node.nodeId })).computers.rename({
          nodeId: node.nodeId,
          displayName: name.trim(),
        });
      } else {
        await (await getTalosClient({ nodeId: node.nodeId })).computers.setStatus({
          nodeId: node.nodeId,
          status: node.status === "enabled" ? "disabled" : "enabled",
        });
      }
    },
    onSuccess: () => queries.invalidateQueries({ queryKey: ["talos", scope] }),
  });
  if (query.error && !node)
    return <ComputerDirectoryError error={query.error} pending={query.isFetching} onRetry={() => void query.refetch()} />;
  if (!node)
    return (
      <p role="status" className="cmp-muted">
        {t("common:loading")}
      </p>
    );
  const peers = computerPeers(metrics, node.nodeId);
  const phase = node.managedWorkerPhase;
  const status = computerStatus(node, local, localRuntime);
  const personal =
    Boolean(directoryNode) && !phase && node.mode === "remote" && node.ownerUserId === connection.userId;
  const canRecover =
    Boolean(getDesktopBridge()) &&
    personal &&
    node.endpoint === null &&
    (!localId || node.nodeId === localId);
  return (
    <>
      {query.error ? <ComputerDirectoryError error={query.error} pending={query.isFetching} onRetry={() => void query.refetch()} /> : null}
      <div className="cmp-inspector-heading">
        <span className="cmp-eyebrow">Computer</span>
        <h2>{node.displayName}</h2>
        <p className="cmp-muted">
          {t(`computers:${status.label}`)}{status.remoteDisabled ? ` · ${t("computers:remoteDisabled")}` : ""}
        </p>
      </div>
      <Button className="w-full" title={!phase && !node.available ? t("computers:offlineHelp") : undefined} onClick={() => onOpen(node.nodeId)}>
        {t("computers:open")}
      </Button>
      <ComputerDesktopButton nodeId={node.nodeId} online={node.available} />
      <SyncPeerDetails
        peers={peers}
        metrics={metrics}
        syncStatus={computerLocalSyncStatus(metrics, node.nodeId, syncStatus)}
      />
      <ComputerDetails node={node} controlPlane={controlPlane} />
      {personal ? (
        <div className="mt-5 space-y-3">
          <label className="block text-xs text-muted">
            {t("computers:rename")}
            <input
              aria-label={t("computers:rename")}
              value={name}
              maxLength={255}
              onChange={(event) => setName(event.target.value)}
              className="mt-2 w-full rounded-lg border border-border bg-background p-2 text-sm text-ink"
            />
          </label>
          <div className="flex flex-wrap gap-2">
            <Button
              disabled={
                !name.trim() ||
                name.trim() === node.displayName ||
                mutation.isPending
              }
              onClick={() => mutation.mutate("rename")}
            >
              {t("common:save")}
            </Button>

          </div>
        </div>
      ) : null}
      {canRecover ? (
        <button
          type="button"
          className="mt-4 text-sm text-brand"
          disabled={mutation.isPending}
          onClick={() => mutation.mutate("recover")}
        >
          {t("computers:recover")}
        </button>
      ) : null}
      {mutation.error ? (
        <p role="alert" className="mt-3 text-sm text-danger">
          {userVisibleErrorMessage(mutation.error, t("computers:loadError"))}
        </p>
      ) : null}
      {personal ? <ComputerRegistrationActions node={node} pending={mutation.isPending} onToggleAccess={() => mutation.mutate("status")} onDeleted={onDeleted} /> : null}
    </>
  );
}
