import { useEffect, useState } from "react";
import { ChevronRight, Edit3, Folder, MoreHorizontal } from "lucide-react";
import { useTranslation } from "react-i18next";

import { cn } from "@/shared/lib/cn";
import type { ProjectView } from "@/shared/types/ui";
import { SidebarProjectActionsMenu } from "./SidebarProjectActionsMenu";
import { SidebarProjectRemoveDialog } from "./SidebarProjectRemoveDialog";
import { SidebarProjectRenameDialog } from "./SidebarProjectRenameDialog";
import { SidebarSectionHeader } from "./SidebarSectionHeader";
import { SidebarSessionRow, type SidebarSessionItem } from "./SidebarSessionRow";

export interface SidebarProjectGroup {
  project: ProjectView;
  sessions: SidebarSessionItem[];
}

interface SidebarProjectSectionProps {
  currentPath: string;
  groups: SidebarProjectGroup[];
  onSelectSession: (sessionId: string) => void;
  onDeleteSession: (session: SidebarSessionItem) => void;
  onCreateProjectThread: (projectId: string) => void;
  onRemoveProject: (projectId: string) => Promise<void> | void;
  onRenameProject: (projectId: string, name: string) => Promise<void> | void;
  projectActionPending?: boolean;
}

export function SidebarProjectSection({
  currentPath,
  groups,
  onSelectSession,
  onDeleteSession,
  onCreateProjectThread,
  onRemoveProject,
  onRenameProject,
  projectActionPending = false,
}: SidebarProjectSectionProps) {
  const { t } = useTranslation(["sidebar", "common"]);
  const [open, setOpen] = useState(true);
  const [expandedProjectIds, setExpandedProjectIds] = useState<Set<string>>(() => new Set());
  const [menuProjectId, setMenuProjectId] = useState<string | null>(null);
  const [renameProject, setRenameProject] = useState<ProjectView | null>(null);
  const [removeProject, setRemoveProject] = useState<ProjectView | null>(null);
  const [expandedSessionProjectIds, setExpandedSessionProjectIds] = useState<Set<string>>(() => new Set());

  useEffect(() => {
    if (!menuProjectId) {
      return;
    }

    function closeMenuOnOutsidePointer(event: PointerEvent) {
      const target = event.target;
      if (!(target instanceof Element)) {
        setMenuProjectId(null);
        return;
      }
      if (
        target.closest("[data-sidebar-project-menu-root]") ||
        target.closest("[data-sidebar-project-menu-trigger]")
      ) {
        return;
      }
      setMenuProjectId(null);
    }

    document.addEventListener("pointerdown", closeMenuOnOutsidePointer, true);
    return () => document.removeEventListener("pointerdown", closeMenuOnOutsidePointer, true);
  }, [menuProjectId]);

  function toggleProject(projectId: string) {
    setExpandedProjectIds((current) => {
      const next = new Set(current);
      if (next.has(projectId)) {
        next.delete(projectId);
      } else {
        next.add(projectId);
      }
      return next;
    });
  }

  return (
    <section className="mt-1">
      <SidebarSectionHeader label={t("projects")} onToggle={() => setOpen((value) => !value)} open={open} />
      {open ? (
        <div className="mt-1 space-y-1">
          {groups.map((group) => {
            const projectOpen = expandedProjectIds.has(group.project.id);
            const projectActive = group.sessions.some((session) => currentPath === `/sessions/${session.id}`);
            return (
              <ProjectGroup
                currentPath={currentPath}
                group={group}
                key={group.project.id}
                menuOpen={menuProjectId === group.project.id}
                onOpenMenu={() => setMenuProjectId(group.project.id)}
                onRemoveProject={() => {
                  setMenuProjectId(null);
                  setRemoveProject(group.project);
                }}
                onCreateProjectThread={() => onCreateProjectThread(group.project.id)}
                onRenameProject={() => {
                  setMenuProjectId(null);
                  setRenameProject(group.project);
                }}
                onShowMoreSessions={() => {
                  setExpandedSessionProjectIds((current) => new Set(current).add(group.project.id));
                }}
                onDeleteSession={onDeleteSession}
                onSelectSession={onSelectSession}
                onToggleProject={() => toggleProject(group.project.id)}
                open={projectOpen}
                projectActive={projectActive}
                sessionsExpanded={expandedSessionProjectIds.has(group.project.id)}
                showMoreLabel={t("common:showMore")}
              />
            );
          })}
        </div>
      ) : null}
      {renameProject ? (
        <SidebarProjectRenameDialog
          initialName={renameProject.name}
          onClose={() => setRenameProject(null)}
          onSave={async (name) => {
            await onRenameProject(renameProject.id, name);
            setRenameProject(null);
          }}
          saving={projectActionPending}
        />
      ) : null}
      {removeProject ? (
        <SidebarProjectRemoveDialog
          onClose={() => setRemoveProject(null)}
          onRemove={async () => {
            await onRemoveProject(removeProject.id);
            setRemoveProject(null);
          }}
          projectName={removeProject.name}
          removing={projectActionPending}
        />
      ) : null}
    </section>
  );
}

function ProjectGroup({
  currentPath,
  group,
  menuOpen,
  onOpenMenu,
  onRemoveProject,
  onCreateProjectThread,
  onRenameProject,
  onShowMoreSessions,
  onDeleteSession,
  onSelectSession,
  onToggleProject,
  open,
  projectActive,
  sessionsExpanded,
  showMoreLabel,
}: {
  currentPath: string;
  group: SidebarProjectGroup;
  menuOpen: boolean;
  onOpenMenu: () => void;
  onRemoveProject: () => void;
  onCreateProjectThread: () => void;
  onRenameProject: () => void;
  onShowMoreSessions: () => void;
  onDeleteSession: (session: SidebarSessionItem) => void;
  onSelectSession: (sessionId: string) => void;
  onToggleProject: () => void;
  open: boolean;
  projectActive: boolean;
  sessionsExpanded: boolean;
  showMoreLabel: string;
}) {
  const visibleSessions = sessionsExpanded ? group.sessions : group.sessions.slice(0, 5);

  return (
    <div>
      <div
        className={cn(
          "group/project relative grid h-9 grid-cols-[minmax(0,1fr)_4.25rem] items-center gap-2 rounded-[var(--radius-control)] px-3 text-sm text-ink transition hover:bg-card/75",
          projectActive && "bg-card/90 hover:bg-card/90",
        )}
      >
        <button
          className="grid min-w-0 grid-cols-[1.25rem_minmax(0,1fr)] items-center gap-2 text-left focus:outline-none focus-visible:ring-2 focus-visible:ring-brand/30"
          onClick={onToggleProject}
          type="button"
        >
          <Folder aria-hidden="true" size={15} />
          <span className="truncate font-medium">{group.project.name}</span>
        </button>
        <span className="flex items-center justify-end gap-1">
          <button
            aria-label="Project actions"
            className="hidden size-5 items-center justify-center rounded text-muted transition hover:bg-background/70 hover:text-ink focus:outline-none focus-visible:ring-2 focus-visible:ring-brand/30 group-hover/project:flex group-focus-within/project:flex"
            data-sidebar-project-menu-trigger=""
            onClick={(event) => {
              event.stopPropagation();
              onOpenMenu();
            }}
            type="button"
          >
            <MoreHorizontal aria-hidden="true" size={14} />
          </button>
          <button
            aria-label="New chat in project"
            className="hidden size-5 items-center justify-center rounded text-muted transition hover:bg-background/70 hover:text-ink focus:outline-none focus-visible:ring-2 focus-visible:ring-brand/30 group-hover/project:flex group-focus-within/project:flex"
            onClick={(event) => {
              event.stopPropagation();
              onCreateProjectThread();
            }}
            type="button"
          >
            <Edit3 aria-hidden="true" size={13} />
          </button>
          <button
            aria-label={open ? "Collapse project" : "Expand project"}
            className="flex size-5 items-center justify-center rounded text-muted transition hover:bg-background/70 hover:text-ink focus:outline-none focus-visible:ring-2 focus-visible:ring-brand/30"
            onClick={(event) => {
              event.stopPropagation();
              onToggleProject();
            }}
            type="button"
          >
            <ChevronRight
              aria-hidden="true"
              className={cn("transition-transform", open && "rotate-90")}
              size={14}
            />
          </button>
        </span>
        {menuOpen ? (
          <SidebarProjectActionsMenu
            onRemove={onRemoveProject}
            onRename={onRenameProject}
          />
        ) : null}
      </div>
      {open ? (
        <div className="mt-1 space-y-1">
          {visibleSessions.map((session) => (
            <SidebarSessionRow
              currentPath={currentPath}
              indent
              key={session.id}
              onDeleteSession={onDeleteSession}
              onSelectSession={onSelectSession}
              session={session}
            />
          ))}
          {group.sessions.length > 5 && !sessionsExpanded ? (
            <button
              className="ml-10 px-3 py-2 text-left text-sm text-muted transition hover:text-ink focus:outline-none focus-visible:ring-2 focus-visible:ring-brand/30"
              onClick={onShowMoreSessions}
              type="button"
            >
              {showMoreLabel}
            </button>
          ) : null}
        </div>
      ) : null}
    </div>
  );
}
