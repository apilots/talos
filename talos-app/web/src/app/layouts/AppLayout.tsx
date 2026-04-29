import { Outlet } from "react-router-dom";

import { SidebarContainer } from "@/features/chat";

export function AppLayout() {
  return (
    <div className="grid h-screen grid-cols-1 overflow-hidden bg-background text-ink lg:grid-cols-[280px_minmax(0,1fr)]">
      <div className="hidden min-w-0 lg:block">
        <SidebarContainer />
      </div>
      <main className="h-screen min-w-0 overflow-hidden">
        <Outlet />
      </main>
    </div>
  );
}
