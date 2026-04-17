import { Outlet } from "react-router-dom";

import { Sidebar } from "@/widgets/sidebar/Sidebar";

export function AppLayout() {
  return (
    <div className="grid h-screen grid-cols-[248px_minmax(0,1fr)] overflow-hidden bg-background text-ink">
      <Sidebar />
      <main className="min-w-0 h-screen overflow-hidden">
        <Outlet />
      </main>
    </div>
  );
}
