import { ArrowLeft, ArrowRight, FileText, PanelLeft, PencilLine } from "lucide-react";
import { Link, useLocation, useNavigate } from "react-router-dom";

import { useSessionsQuery } from "@/features/chat";
import { useAppStore } from "@/shared/store/app-store";

interface CollapsedSidebarBarProps {
  onOpenSidebar: () => void;
}

function staticPageTitle(pathname: string) {
  if (pathname === "/") {
    return "";
  }
  if (pathname.startsWith("/plugins")) {
    return "Plugins";
  }
  if (pathname.startsWith("/files")) {
    return "Files";
  }
  if (pathname.startsWith("/memory")) {
    return "Search";
  }
  if (pathname.startsWith("/settings")) {
    return "Settings";
  }
  return "";
}

export function CollapsedSidebarBar({ onOpenSidebar }: CollapsedSidebarBarProps) {
  const location = useLocation();
  const navigate = useNavigate();
  const sessionsQuery = useSessionsQuery();
  const projects = useAppStore((state) => state.projects);
  const sessionId = location.pathname.startsWith("/sessions/")
    ? location.pathname.slice("/sessions/".length)
    : null;
  const session = sessionId ? sessionsQuery.data?.find((entry) => entry.id === sessionId) : undefined;
  const project = session?.project_id ? projects.find((entry) => entry.id === session.project_id) : undefined;
  const title = session?.title ?? staticPageTitle(location.pathname);

  return (
    <div className="flex h-12 shrink-0 items-center gap-2 border-b border-border/55 bg-background px-4">
      <button
        aria-label="Open sidebar"
        className="flex size-8 items-center justify-center rounded-[var(--radius-control)] border border-border/70 bg-surface text-muted transition hover:text-ink focus:outline-none focus-visible:ring-2 focus-visible:ring-brand/30"
        onClick={onOpenSidebar}
        type="button"
      >
        <PanelLeft aria-hidden="true" size={16} />
      </button>
      <button
        aria-label="Back"
        className="flex size-8 items-center justify-center rounded-[var(--radius-control)] text-muted transition hover:bg-surface hover:text-ink focus:outline-none focus-visible:ring-2 focus-visible:ring-brand/30"
        onClick={() => navigate(-1)}
        type="button"
      >
        <ArrowLeft aria-hidden="true" size={17} />
      </button>
      <button
        aria-label="Forward"
        className="flex size-8 items-center justify-center rounded-[var(--radius-control)] text-muted transition hover:bg-surface hover:text-ink focus:outline-none focus-visible:ring-2 focus-visible:ring-brand/30"
        onClick={() => navigate(1)}
        type="button"
      >
        <ArrowRight aria-hidden="true" size={17} />
      </button>
      <Link
        aria-label="New chat"
        className="flex size-8 items-center justify-center rounded-[var(--radius-control)] text-muted transition hover:bg-surface hover:text-ink focus:outline-none focus-visible:ring-2 focus-visible:ring-brand/30"
        to="/"
      >
        <PencilLine aria-hidden="true" size={17} />
      </Link>
      {title ? (
        <div className="ml-1 flex min-w-0 items-center gap-2 text-sm font-semibold text-ink">
          <FileText aria-hidden="true" className="shrink-0 text-muted" size={16} />
          {project ? (
            <>
              <span className="max-w-[180px] truncate text-muted">{project.name}</span>
              <span aria-hidden="true" className="text-muted/55">/</span>
            </>
          ) : null}
          <span className="max-w-[520px] truncate">{title}</span>
        </div>
      ) : null}
    </div>
  );
}
