import { selectionClasses } from "@/shared/ui/controlStyles";
import { Check, Laptop, Monitor } from "lucide-react";
import { useTranslation } from "react-i18next";
import type { TalosRuntimeState } from "@talos/sdk-app-server/desktop";
import type { AppProtocol } from "@talos/sdk-core";
import { computerStatus } from "./computer-status";

export function ComputerItem({ node, local = false, localRuntime, selected = false, compact = false, onSelect }: {
  node: AppProtocol.NodeInfo; local?: boolean; localRuntime?: TalosRuntimeState; selected?: boolean; compact?: boolean; onSelect: () => void;
}) {
  const { t } = useTranslation("computers");
  const Icon = local ? Laptop : Monitor;
  const status = computerStatus(node, local, localRuntime);
  const statusLabel = `${t(status.label)}${status.remoteDisabled ? ` · ${t("remoteDisabled")}` : ""}`;
  const label = `${node.displayName}${local ? ` · ${t("local")}` : ""} · ${statusLabel}`;
  return <button type="button" title={label} aria-label={label} aria-pressed={selected}
    data-computer-id={node.nodeId} onClick={onSelect}
    className={`relative flex items-center rounded-xl transition focus-visible:outline focus-visible:outline-2 focus-visible:outline-brand ${compact ? "size-10 justify-center" : "w-full gap-3 p-3 text-left"} ${selected ? selectionClasses.selected : selectionClasses.idle}`}>
    <span className="relative shrink-0"><Icon size={compact ? 21 : 20} /><span className={`absolute -bottom-1 -right-1 size-2 rounded-full border-2 border-sidebar ${status.ready ? "bg-success" : "bg-muted"}`} /></span>
    {!compact ? <><span className="min-w-0 flex-1"><span className="flex items-center gap-2"><span className="truncate text-sm font-medium text-ink">{node.displayName}</span>{local ? <span className="shrink-0 rounded bg-surface px-1 text-[10px] text-muted">{t("local")}</span> : null}</span><span className="block text-xs text-muted">{statusLabel}</span></span>{selected ? <Check size={16} /> : null}</> : null}
  </button>;
}
