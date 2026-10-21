import type { CSSProperties, ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { AppTopBar } from "./AppTopBar";
import { AppResizeHandle } from "./AppResizeHandle";
import { SIDEBAR_WIDTH } from "@/shared/store/ui-slice";
import { useAppStore } from "@/shared/store/app-store";

const columns = "grid grid-cols-[var(--sidebar-width)_minmax(0,1fr)] transition-[grid-template-columns] duration-200 ease-out motion-reduce:transition-none";

/** One width owns the middle split; one navigation DOM stays reachable through its drawer transition. */
export function AppBusinessRegion({ inactive, headerVisible, sidebarExpanded, sidebarWidth, sidebarMax, navigation, mainHeader, sidebar, children }: {
  inactive: boolean; headerVisible: boolean; sidebarExpanded: boolean; sidebarWidth: number; sidebarMax: number; navigation?: ReactNode; mainHeader: ReactNode; sidebar?: ReactNode; children: ReactNode;
}) {
  const { t } = useTranslation("common");
  const resizing = useAppStore(state => state.resizingPane !== null);
  const setWidth = useAppStore(state => state.setSidebarWidth);
  const setResizing = useAppStore(state => state.setResizingPane);
  const tracks = resizing ? "grid grid-cols-[var(--sidebar-width)_minmax(0,1fr)]" : columns;
  return <section inert={inactive} aria-hidden={inactive} aria-label={t("navigation.business")} style={{ "--sidebar-width": sidebarExpanded ? `${sidebarWidth}px` : "0px" } as CSSProperties} className="flex h-full min-w-0 flex-1 flex-col bg-surface">
    {headerVisible ? <div className={`${tracks} h-12 shrink-0`}>
      <div className={`relative z-10 min-w-0 ${sidebarExpanded ? "border-r border-border/70" : ""}`}>
        {sidebarExpanded && !inactive ? <AppResizeHandle label={t("navigation.resizeSidebar")} controls="talos-sidebar" value={sidebarWidth} min={SIDEBAR_WIDTH.min} max={sidebarMax} onResize={setWidth} onDragging={dragging => setResizing(dragging ? "sidebar" : null)} className="-right-1 top-0 h-screen" /> : null}
        {navigation ? <AppTopBar label={t("navigation.sidebarControls")} className="h-full border-b-0"><div className="absolute inset-y-0 left-2 flex items-center sm:left-4">{navigation}</div></AppTopBar> : null}
      </div>
      <div className={`min-w-0 ${resizing ? "" : "transition-[padding-left] duration-200 ease-out motion-reduce:transition-none"} ${sidebarExpanded ? "pl-0" : "pl-36 sm:pl-40"}`}>{mainHeader}</div>
    </div> : null}
    <div className={`${tracks} min-h-0 flex-1 overflow-hidden`}>
      <div inert={!sidebarExpanded} aria-hidden={!sidebarExpanded} className={`min-w-0 overflow-hidden rounded-lg bg-sidebar ${sidebarExpanded ? "border border-border/50" : ""}`}>{sidebar}</div>
      {children}
    </div>
  </section>;
}
