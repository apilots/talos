import { selectionClasses } from "@/shared/ui/controlStyles";
import { Check, Laptop, Monitor } from "lucide-react";
import { useTranslation } from "react-i18next";
import type { AppProtocol } from "@talos/sdk-core";

export function ComputerItem({ node, local = false, selected = false, compact = false, onSelect }: {
  node: AppProtocol.NodeInfo; local?: boolean; selected?: boolean; compact?: boolean; onSelect: () => void;
}) {
  const { t } = useTranslation("computers");
  const Icon = local ? Laptop : Monitor;
  const status = node.status === "disabled" ? (local && node.available ? "remoteDisabled" : "disabled") : node.available ? "connected" : "offline";
  const label = `${node.displayName}${local ? ` · ${t("local")}` : ""} · ${t(status)}`;
  return <button type="button" title={label} aria-label={label} aria-pressed={selected}
    data-computer-id={node.nodeId} onClick={onSelect}
    className={`relative flex items-center rounded-xl transition focus-visible:outline focus-visible:outline-2 focus-visible:outline-brand ${compact ? "size-10 justify-center" : "w-full gap-3 p-3 text-left"} ${selected ? selectionClasses.selected : selectionClasses.idle}`}>
    <span className="relative shrink-0"><Icon size={compact ? 21 : 20} /><span className={`absolute -bottom-1 -right-1 size-2 rounded-full border-2 border-sidebar ${node.available ? "bg-success" : "bg-muted"}`} /></span>
    {!compact ? <><span className="min-w-0 flex-1"><span className="flex items-center gap-2"><span className="truncate text-sm font-medium text-ink">{node.displayName}</span>{local ? <span className="shrink-0 rounded bg-surface px-1 text-[10px] text-muted">{t("local")}</span> : null}</span><span className="block text-xs text-muted">{t(status)}</span></span>{selected ? <Check size={16} /> : null}</> : null}
  </button>;
}
