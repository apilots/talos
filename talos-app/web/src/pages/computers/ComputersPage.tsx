import { useEffect, useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router-dom";
import { createElectronAppServerClient } from "@talos/sdk-app-server/desktop";
import { ComputerDetails, DesktopConnectionControl, ComputerItem, useComputerList, useComputers } from "@/features/computers";
import { getTalosClient } from "@/shared/api/client";
import { getDesktopBridge } from "@/shared/api/desktop-client";
import { SyncControl } from "@/features/computers";
import { useAppStore } from "@/shared/store/app-store";
import { apiErrorMessage, userVisibleErrorMessage } from "@/shared/i18n/apiErrorMessage";

export function ComputersPage() {
  const { t } = useTranslation(["computers", "common"]);
  const computers = useComputers();
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const [search, setSearch] = useState("");
  const [name, setName] = useState("");
  const [showHelp, setShowHelp] = useState(false);
  const list = useComputerList(search.trim());
  const connection = useAppStore((state) => state.connection);
  const node = computers.selected;
  useEffect(() => { setName(node?.displayName ?? ""); }, [node?.nodeId, node?.displayName]);
  const mutation = useMutation({ mutationFn: async (action: "rename" | "status" | "recover") => {
    const client = getTalosClient();
    if (action === "recover") {
      const bridge = getDesktopBridge();
      if (bridge) await createElectronAppServerClient(bridge).desktop.enrollWorker({ nodeId: computers.localId ?? node?.nodeId ?? null });
    } else if (node) {
      if (action === "rename") await client.computers.rename({ nodeId: node.nodeId, displayName: name.trim() });
      else await client.computers.setStatus({ nodeId: node.nodeId, status: node.status === "enabled" ? "disabled" : "enabled" });
    }
  }, onSuccess: async () => { await queryClient.invalidateQueries({ queryKey: ["talos", computers.scope] }); } });
  const personal = node?.mode === "remote" && node.ownerUserId === connection.userId;
  const canRecover = connection.desktopConnectionMode === "cloud" && personal && node.endpoint === null && (!computers.localId || node.nodeId === computers.localId);
  return <div className="h-full overflow-auto px-6 py-8 lg:px-12">
    <div className="mx-auto max-w-4xl">
      <div className="mb-6 flex items-center justify-between"><h1 className="text-2xl font-semibold">{t("computers:title")}</h1><button className="rounded-lg border border-border px-3 py-2 text-sm" onClick={() => setShowHelp(!showHelp)}>{t("computers:add")}</button></div>
      {showHelp ? <p className="mb-6 rounded-xl border border-border bg-card p-4 text-sm text-muted">{t("computers:addHelp")}</p> : null}
      <SyncControl />
      <DesktopConnectionControl />
      {computers.runtime?.error ? <p role="alert" className="mb-4 text-sm text-danger">{apiErrorMessage(computers.runtime.error)}</p> : null}
      {connection.desktopConnectionMode === "cloud" && !computers.localId ? <p className="mb-4 text-sm text-muted">{t("computers:recoverHelp")}</p> : null}
      <div className="grid gap-8 md:grid-cols-[minmax(240px,1fr)_1fr]">
        <section><input aria-label={t("computers:search")} placeholder={t("computers:search")} value={search} onChange={(event) => setSearch(event.target.value)} className="mb-3 w-full rounded-lg border border-border bg-card px-3 py-2 text-sm" />
          {list.data?.pages.flatMap((page) => page.data).map((item) => <ComputerItem key={item.nodeId} node={item} selected={item.nodeId === node?.nodeId} local={item.nodeId === computers.localId} onSelect={() => computers.select(item.nodeId)} />)}
          {list.hasNextPage ? <button disabled={list.isFetchingNextPage} className="p-3 text-sm text-brand" onClick={() => void list.fetchNextPage()}>{t("common:showMore")}</button> : null}
          {list.error ? <p role="alert" className="py-3 text-sm text-danger">{userVisibleErrorMessage(list.error, t("computers:loadError"))}</p> : null}
        </section>
        {node ? <section className="rounded-2xl border border-border bg-card p-5">
          <h2 className="mb-4 truncate text-lg font-semibold">{node.displayName}</h2>
          <p className="mb-2 text-xs text-muted">{t(node.status === "disabled" ? (node.nodeId === computers.localId && node.available ? "computers:remoteDisabled" : "computers:disabled") : node.available ? "computers:connected" : "computers:offline")}</p>
          <ComputerDetails node={node} controlPlane={connection.controlPlaneEndpoint || window.location.origin} />
          {!node.available ? <p className="mt-4 text-sm text-muted">{t("computers:offlineHelp")}</p> : null}
          {personal ? <div className="mt-6 space-y-3"><label className="block text-xs text-muted">{t("computers:rename")}<input aria-label={t("computers:rename")} value={name} maxLength={255} onChange={(event) => setName(event.target.value)} className="mt-1 w-full rounded-lg border border-border bg-background p-2 text-sm text-ink" /></label><div className="flex flex-wrap gap-2"><button className="rounded-lg bg-brand px-3 py-2 text-sm text-background" disabled={!name.trim() || mutation.isPending} onClick={() => mutation.mutate("rename")}>{t("common:save")}</button><button className="rounded-lg border border-border px-3 py-2 text-sm" disabled={mutation.isPending} onClick={() => mutation.mutate("status")}>{t(node.status === "enabled" ? "computers:disable" : "computers:enable")}</button></div></div> : null}
          {canRecover ? <button className="mt-4 block text-sm text-brand" disabled={mutation.isPending} onClick={() => mutation.mutate("recover")}>{t("computers:recover")}</button> : null}
          {mutation.error ? <p role="alert" className="mt-3 text-sm text-danger">{userVisibleErrorMessage(mutation.error, t("computers:loadError"))}</p> : null}
          <button className="mt-6 text-sm text-brand" onClick={() => navigate("/")}>{t("computers:open")}</button>
        </section> : null}
      </div>
    </div>
  </div>;
}
