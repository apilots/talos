import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { MonitorCog, Search } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Link } from "react-router-dom";
import { userVisibleErrorMessage } from "@/shared/i18n/apiErrorMessage";
import { ComputerItem } from "./ComputerItem";
import { useComputerList } from "./hooks";

export function ComputerOverflowMenu({ selectedId, localId, onSelect, onClose }: {
  selectedId?: string; localId?: string; onSelect: (id: string) => void; onClose: () => void;
}) {
  const { t } = useTranslation(["computers", "common"]);
  const [search, setSearch] = useState("");
  const list = useComputerList(search.trim());
  const ref = useRef<HTMLDivElement>(null);
  const [offset, setOffset] = useState(0);
  useLayoutEffect(() => {
    const fit = () => {
      const menu = ref.current;
      if (!menu) return;
      const anchor = menu.parentElement!.getBoundingClientRect().top;
      setOffset(-Math.max(0, anchor + menu.offsetHeight - window.innerHeight + 12));
    };
    fit(); window.addEventListener("resize", fit);
    return () => window.removeEventListener("resize", fit);
  }, [list.data, list.isPending]);
  useEffect(() => {
    const dismiss = (event: PointerEvent) => { if (!ref.current?.parentElement?.contains(event.target as Node)) onClose(); };
    document.addEventListener("pointerdown", dismiss);
    return () => document.removeEventListener("pointerdown", dismiss);
  }, [onClose]);
  return <div ref={ref} style={{ top: offset }} role="dialog" aria-label={t("computers:title")} onKeyDown={(event) => {
    if (event.key === "Escape") { event.stopPropagation(); onClose(); }
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      const controls = Array.from(ref.current?.querySelectorAll<HTMLElement>("input,button,a") ?? []);
      const index = controls.indexOf(document.activeElement as HTMLElement);
      controls[(index + (event.key === "ArrowDown" ? 1 : controls.length - 1)) % controls.length]?.focus(); event.preventDefault();
    }
  }} className="absolute left-[calc(100%+8px)] top-0 z-50 flex max-h-[calc(100vh-24px)] w-[min(360px,calc(100vw-76px))] flex-col overflow-hidden rounded-2xl border border-border bg-card p-2 shadow-panel">
    <div className="flex items-center justify-between px-3 py-2 text-sm font-semibold">{t("computers:title")}<span className="text-xs font-normal text-muted">{list.data?.pages[0]?.total ?? 0}</span></div>
    <label className="m-1 flex items-center gap-2 rounded-lg border border-border px-3 py-2"><Search size={15} className="text-muted" /><input autoFocus aria-label={t("computers:search")} placeholder={t("computers:search")} value={search} onChange={(event) => setSearch(event.target.value)} className="min-w-0 flex-1 bg-transparent text-sm outline-none" /></label>
    <div className="min-h-0 max-h-[55vh] overflow-auto py-1">
      {list.data?.pages.flatMap((page) => page.data).map((node) => <ComputerItem key={node.nodeId} node={node} local={node.nodeId === localId} selected={node.nodeId === selectedId} onSelect={() => { onSelect(node.nodeId); onClose(); }} />)}
      {list.isPending ? <p className="p-3 text-sm text-muted">{t("common:loading")}</p> : null}
      {list.error ? <p role="alert" className="p-3 text-sm text-danger">{userVisibleErrorMessage(list.error, t("computers:loadError"))}</p> : null}
      {list.data?.pages[0]?.total === 0 ? <p className="p-3 text-sm text-muted">{t("computers:empty")}</p> : null}
      {list.hasNextPage ? <button type="button" disabled={list.isFetchingNextPage} onClick={() => void list.fetchNextPage()} className="w-full p-2 text-sm text-brand">{t("common:showMore")}</button> : null}
    </div>
    <Link onClick={onClose} to="/computers" className="flex items-center gap-2 border-t border-border px-3 py-3 text-sm"><MonitorCog size={16} />{t("computers:manage")}</Link>
  </div>;
}
