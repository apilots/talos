import { Outlet } from "react-router-dom";

import { SidebarContainer } from "@/features/chat";

export function AppLayout() {
  return (
    <div className="grid h-screen grid-cols-[280px_minmax(0,1fr)] overflow-hidden bg-background text-ink">
      <SidebarContainer />
      <main className="h-screen min-w-0 overflow-hidden">
        <Outlet />
      </main>
    </div>
  );
}
