import type { ReactNode, Ref } from "react";
import { ArrowLeft, ArrowRight, FileText, Maximize2, Minimize2, PanelLeft, PanelRight, PencilLine, Plus } from "lucide-react";
import { useTranslation } from "react-i18next";
import { useAppStore } from "@/shared/store/app-store";
import { Button } from "@/shared/ui/Button";
import { cn } from "@/shared/lib/cn";
import { Link, useLocation, useNavigate } from "react-router-dom";
import { useProjectsQuery, useSessionQuery } from "@/features/chat";
import { normalizeProjects } from "@/shared/store/ui-slice";

/** Middle-region navigation and Session context own no workspace geometry. */
export function AppNavigation({ sidebarAvailable, sidebarToggleRef, sidebarVisible, onToggleSidebar }: {
  sidebarAvailable: boolean; sidebarVisible: boolean; sidebarToggleRef: Ref<HTMLButtonElement>; onToggleSidebar: () => void;
}) {
  const { t } = useTranslation("common");
  const navigate = useNavigate();
  return <div className="flex shrink-0 items-center gap-1.5 sm:gap-3">
    {sidebarAvailable ? <Button ref={sidebarToggleRef} variant="ghost" className="p-2" aria-label={t(sidebarVisible ? "navigation.hideSidebar" : "navigation.showSidebar")} aria-controls="talos-sidebar" aria-expanded={sidebarVisible} onClick={onToggleSidebar}><PanelLeft aria-hidden="true" size={16} /></Button> : null}
    <div role="group" aria-label={t("navigation.history")} className="flex shrink-0 items-center gap-0.5">
      <Button variant="ghost" className="p-2" aria-label={t("navigation.back")} onClick={() => navigate(-1)}><ArrowLeft aria-hidden="true" size={16} /></Button>
      <Button variant="ghost" className="p-2" aria-label={t("navigation.forward")} onClick={() => navigate(1)}><ArrowRight aria-hidden="true" size={16} /></Button>
      <Link to="/" aria-label={t("navigation.newChat")} className="flex size-8 items-center justify-center rounded-lg text-muted hover:bg-surface hover:text-ink focus-visible:outline focus-visible:outline-2 focus-visible:outline-brand"><PencilLine aria-hidden="true" size={16} /></Link>
    </div>

  </div>;
}

/** Session/project context and Computer actions belong to the main business column. */
export function AppSessionContext() {
  const { t } = useTranslation("common");
  const location = useLocation();
  const navigate = useNavigate();
  const setActiveProject = useAppStore(state => state.setActiveProjectId);
  const sessionId = location.pathname.startsWith("/sessions/") ? location.pathname.slice("/sessions/".length) : "";
  // The Session page owns loading/admission; navigation only observes its cache.
  const { data: session } = useSessionQuery(sessionId, false);
  const { data: projects } = useProjectsQuery(session?.nodeId);
  const project = session?.projectId ? normalizeProjects(projects).find(entry => entry.id === session.projectId) : undefined;
  const page = location.pathname.startsWith("/settings") ? "settings" : location.pathname.startsWith("/computers") ? "computers"
    : location.pathname.startsWith("/plugins") ? "plugins" : location.pathname.startsWith("/files") ? "files"
    : location.pathname.startsWith("/memory") ? "memory" : "newChat";
  const title = session?.title ?? t(`navigation.${page}`);
  return <div className="flex min-w-0 flex-1 items-center gap-2">
    <div className="flex min-w-0 flex-1 items-center gap-2 text-xs">
      <FileText aria-hidden="true" size={14} className="hidden shrink-0 text-muted sm:block" />
      {project ? <><button type="button" className="hidden max-w-40 truncate text-muted hover:text-ink sm:block" onClick={() => { setActiveProject(project.id); navigate("/"); }}>{project.name}</button><span aria-hidden="true" className="hidden text-muted/40 sm:block">/</span></> : null}
      <span className="truncate font-medium text-ink" title={title}>{title}</span>
    </div>
  </div>;
}

/** A header belongs to one full-height region, with its own vertical boundary. */
export function AppTopBar({ children, label, className }: { children: ReactNode; label?: string; className?: string }) {
  const { t } = useTranslation("common");
  return <header aria-label={label ?? t("workspace.navigation")} className={cn("flex h-12 shrink-0 items-center gap-2 border-b border-border/50 bg-surface px-2 sm:px-4", className)}>{children}</header>;
}

/** Geometry/count controls know no Computer authority or Session context. */
export function WorkspaceControls({ toggleRef }: { toggleRef: Ref<HTMLButtonElement> }) {
  const { t } = useTranslation("common");
  const windows = useAppStore(state => state.workspaceWindows);
  const visible = useAppStore(state => state.workspaceOpen);
  const expanded = useAppStore(state => state.workspaceExpanded);
  const setOpen = useAppStore(state => state.setWorkspaceOpen);
  const setExpanded = useAppStore(state => state.setWorkspaceExpanded);
  const createWebpage = useAppStore(state => state.createWorkspaceWebpage);
  const toggleLabel = windows.length ? t(visible ? "workspace.hide" : "workspace.show", { count: windows.length }) : t("workspace.create");
  return <div role="group" aria-label={t("workspace.controls")} className="ml-auto flex shrink-0 items-center gap-1">
    <Button variant="ghost" className="p-2" disabled={!visible} aria-label={t(expanded ? "workspace.restore" : "workspace.expand")} title={t(expanded ? "workspace.restore" : "workspace.expand")} onClick={() => setExpanded(!expanded)}>
      {expanded ? <Minimize2 aria-hidden="true" size={16} /> : <Maximize2 aria-hidden="true" size={16} />}
    </Button>
    <Button ref={toggleRef} id="talos-workspace-toggle" variant="ghost" className="gap-1.5 p-2" aria-controls="talos-right-workspace" aria-expanded={visible} aria-label={toggleLabel} title={toggleLabel} onClick={() => windows.length ? setOpen(!visible) : createWebpage()}>
      {windows.length ? <PanelRight aria-hidden="true" size={17} /> : <Plus aria-hidden="true" size={17} />}
      {windows.length > 0 ? <span className="min-w-4 rounded bg-selected px-1 text-[10px] leading-4 tabular-nums text-ink" aria-hidden="true">{windows.length}</span> : null}
    </Button>
  </div>;
}
