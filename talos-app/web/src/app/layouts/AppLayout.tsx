import { useTranslation } from "react-i18next";
import { useEffect, useRef } from "react";
import { Outlet, useLocation } from "react-router-dom";

import { ComputerRail, useComputers, useSelectedComputerId, useProductScope } from "@/features/computers";
import { SidebarContainer } from "@/features/chat";
import { useAppStore } from "@/shared/store/app-store";
import { CollapsedSidebarBar } from "./CollapsedSidebarBar";

export function AppLayout() {
  const location = useLocation();
  const { t } = useTranslation("computers");
  const { selected: computer } = useComputers();
  const computerManagement = location.pathname === "/computers";
  const computerId = useSelectedComputerId();
  const scope = useProductScope();
  const sidebarOpen = useAppStore((state) => state.sidebarOpen);
  const setSidebarOpen = useAppStore((state) => state.setSidebarOpen);
  const settingsRoute = location.pathname.startsWith("/settings");
  const showMainSidebar = sidebarOpen && !settingsRoute && !computerManagement;
  const sidebar = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const dialog = sidebar.current;
    if (!showMainSidebar || !dialog) return;
    const viewport = window.matchMedia("(min-width: 1024px)");
    const focused = document.activeElement;
    const returnFocus = focused instanceof HTMLElement && focused !== document.body && !dialog.contains(focused)
      ? focused : document.querySelector<HTMLButtonElement>('button[aria-controls="talos-sidebar"]');
    let modal = false;
    const display = () => {
      dialog.close();
      modal = !viewport.matches;
      if (viewport.matches) dialog.show();
      else dialog.showModal();
    };
    display();
    viewport.addEventListener("change", display);
    return () => { viewport.removeEventListener("change", display); dialog.close(); if (modal && returnFocus?.isConnected) returnFocus.focus(); };
  }, [showMainSidebar]);

  return (
    <div
      className={
        showMainSidebar
          ? "grid h-screen overflow-hidden bg-background text-ink grid-cols-[56px_minmax(0,1fr)] lg:grid-cols-[56px_296px_minmax(0,1fr)]"
          : "grid h-screen grid-cols-[56px_minmax(0,1fr)] overflow-hidden bg-background text-ink"
      }
    >
      <ComputerRail />
      {showMainSidebar ? (
        <dialog id="talos-sidebar" ref={sidebar} aria-label="Sidebar" onCancel={() => setSidebarOpen(false)}
          onClick={(event) => {
            if (event.target !== event.currentTarget) return;
            const bounds = event.currentTarget.getBoundingClientRect();
            if (event.clientX < bounds.left || event.clientX > bounds.right || event.clientY < bounds.top || event.clientY > bounds.bottom) setSidebarOpen(false);
          }}
          className="fixed inset-y-0 left-14 m-0 h-screen max-h-none w-[min(296px,calc(100vw-56px))] max-w-none min-w-0 flex-col border-0 bg-sidebar p-0 text-ink shadow-xl backdrop:bg-black/30 open:flex lg:static lg:w-auto lg:shadow-none">
          <div className="min-h-0 flex-1 overflow-hidden"><SidebarContainer computer={computer} /></div>
        </dialog>
      ) : null}
      <main className="relative flex h-screen min-w-0 flex-col overflow-hidden">
        {!sidebarOpen && !settingsRoute && !computerManagement && !computerManagement ? <CollapsedSidebarBar onOpenSidebar={() => setSidebarOpen(true)} /> : null}
        <div className="min-h-0 flex-1 overflow-hidden">
          <div key={`${scope}:${location.pathname.startsWith("/sessions/") ? "session" : computerId ?? "pending"}`} className="h-full">{computerId || location.pathname === "/computers" || location.pathname.startsWith("/sessions/") ? <Outlet /> : <div className="flex h-full items-center justify-center text-sm text-muted">{t("choose")}</div>}</div>
        </div>
      </main>
    </div>
  );
}
