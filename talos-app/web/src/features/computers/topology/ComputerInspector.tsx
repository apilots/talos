import { useEffect, useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { createElectronAppServerClient } from "@talos/sdk-app-server/desktop";
import type { AppProtocol } from "@talos/sdk-core";
import { getTalosClient } from "@/shared/api/client";
import { getDesktopBridge } from "@/shared/api/desktop-client";
import { useAppStore } from "@/shared/store/app-store";
import { userVisibleErrorMessage } from "@/shared/i18n/apiErrorMessage";
import { Button } from "@/shared/ui/Button";
import { useComputer, useProductScope } from "../hooks";
import { ComputerDetails } from "../ComputerDetails";
import { ComputerDesktopButton } from "../ComputerDesktop";
import { SyncPeerDetails } from "./SyncPeerDetails";
import { ComputerRegistrationActions } from "./ComputerRegistrationActions";
import { computerPeers } from "./observations";

interface Props {
  nodeId: string;
  localId?: string;
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
  controlPlane,
  metrics,
  syncStatus,
  onOpen,
  onDeleted,
}: Props) {
  const { t } = useTranslation(["computers", "common"]);
  const query = useComputer(nodeId);
  const node = query.data;
  const scope = useProductScope();
  const connection = useAppStore((state) => state.connection);
  const queries = useQueryClient();
  const [name, setName] = useState("");
  useEffect(() => {
    setName(node?.displayName ?? "");
  }, [node?.nodeId, node?.displayName]);
  const mutation = useMutation({
    mutationFn: async (action: "rename" | "status" | "recover") => {
      if (!node) return;
      if (action === "recover") {
        const bridge = getDesktopBridge();
        if (bridge)
          await createElectronAppServerClient(bridge).desktop.enrollWorker({
            nodeId: localId ?? node.nodeId,
          });
      } else if (action === "rename") {
        await getTalosClient().computers.rename({
          nodeId: node.nodeId,
          displayName: name.trim(),
        });
      } else {
        await getTalosClient().computers.setStatus({
          nodeId: node.nodeId,
          status: node.status === "enabled" ? "disabled" : "enabled",
        });
      }
    },
    onSuccess: () => queries.invalidateQueries({ queryKey: ["talos", scope] }),
  });
  if (query.error)
    return (
      <p role="alert" className="text-sm text-danger">
        {userVisibleErrorMessage(query.error, t("computers:loadError"))}
      </p>
    );
  if (!node)
    return (
      <p role="status" className="cmp-muted">
        {t("common:loading")}
      </p>
    );
  const peers = computerPeers(metrics, node.nodeId);
  const personal =
    node.mode === "remote" && node.ownerUserId === connection.userId;
  const canRecover =
    Boolean(getDesktopBridge()) &&
    personal &&
    node.endpoint === null &&
    (!localId || node.nodeId === localId);
  return (
    <>
      <div className="cmp-inspector-heading">
        <span className="cmp-eyebrow">Computer</span>
        <h2>{node.displayName}</h2>
        <p className="cmp-muted">
          {t(
            node.status === "disabled"
              ? "computers:remoteDisabled"
              : node.available
                ? "computers:connected"
                : "computers:offline",
          )}
        </p>
      </div>
      <Button className="w-full" onClick={() => onOpen(node.nodeId)}>
        {t("computers:open")}
      </Button>
      {!node.available ? (
        <p className="cmp-footnote">{t("computers:offlineHelp")}</p>
      ) : null}
      <ComputerDesktopButton nodeId={node.nodeId} online={node.available} />
      <SyncPeerDetails
        peers={peers}
        metrics={metrics}
        syncStatus={
          syncStatus?.localComputerId === node.nodeId ||
          peers.some((peer) => peer.peerId === syncStatus?.peerId)
            ? syncStatus
            : undefined
        }
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
