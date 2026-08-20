import { Outlet, useLocation } from "react-router-dom";

import { SidebarContainer } from "@/features/chat";
import { useAppStore } from "@/shared/store/app-store";
import { CollapsedSidebarBar } from "./CollapsedSidebarBar";

export function AppLayout() {
  const location = useLocation();
  const sidebarOpen = useAppStore((state) => state.sidebarOpen);
  const setSidebarOpen = useAppStore((state) => state.setSidebarOpen);
  const settingsRoute = location.pathname.startsWith("/settings");
  const showMainSidebar = sidebarOpen && !settingsRoute;

  return (
    <div
      className={
        showMainSidebar
          ? "grid h-screen grid-cols-1 overflow-hidden bg-background text-ink lg:grid-cols-[320px_minmax(0,1fr)]"
          : "grid h-screen grid-cols-1 overflow-hidden bg-background text-ink"
      }
    >
      {showMainSidebar ? (
        <div className="hidden min-w-0 lg:block">
          <SidebarContainer />
        </div>
      ) : null}
      <main className="relative flex h-screen min-w-0 flex-col overflow-hidden">
        {!sidebarOpen && !settingsRoute ? <CollapsedSidebarBar onOpenSidebar={() => setSidebarOpen(true)} /> : null}
        <div className="min-h-0 flex-1 overflow-hidden">
          <Outlet />
        </div>
      </main>
    </div>
  );
}
