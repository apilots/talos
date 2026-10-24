import { selectionClasses } from "@/shared/ui/controlStyles";
import { useEffect, useRef, useState } from "react";
import { ArrowLeft, ChevronDown, FolderOpen, FolderPlus, Plus, Search, TriangleAlert } from "lucide-react";
import { useTranslation } from "react-i18next";

import { createFsDirectory, readFsDirectory } from "../api";
import { ProjectDirectoryDialog, type ProjectDirectoryDialogMode } from "@/features/projects/components/ProjectDirectoryDialog";
import { cn } from "@/shared/lib/cn";
import { describeProjectDirectory, isUnboundProject } from "@/shared/types/domain";
import type { ProjectView } from "@/shared/types/ui";
import { filterComposerProjects } from "./ChatComposer.utils";

interface ComposerProjectContextProps {
  activeProjectId: string;
  compact: boolean;
  onCreateProject?: (input: string | { directory: string; name?: string }) => Promise<void>;
  onProjectChange: (projectId: string) => void;
  projects: ProjectView[];
  showProjectSelector: boolean;
}

export function ComposerProjectContext({
  activeProjectId,
  compact,
  onCreateProject,
  onProjectChange,
  projects,
  showProjectSelector,
}: ComposerProjectContextProps) {
  const { t } = useTranslation(["chat", "common"]);
  const [open, setOpen] = useState(false);
  const [search, setSearch] = useState("");
  const [mode, setMode] = useState<"list" | "add_menu">("list");
  const [directoryDialogMode, setDirectoryDialogMode] = useState<ProjectDirectoryDialogMode | null>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const pickerRef = useRef<HTMLDivElement>(null);
  const activeProject = projects.find((project) => project.id === activeProjectId);
  const isProjectBound = activeProject && !isUnboundProject(activeProject.id);
  const filteredProjects = filterComposerProjects(projects, search);
  const collapsible = showProjectSelector && !compact;

  const close = () => {
    setOpen(false);
    setSearch("");
    setMode("list");
  };

  useEffect(() => {
    if (!open) return;
    const onPointerDown = (event: PointerEvent) => {
      const target = event.target as Node | null;
      if (!target || triggerRef.current?.contains(target) || pickerRef.current?.contains(target)) return;
      close();
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") close();
    };
    document.addEventListener("pointerdown", onPointerDown);
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [open]);

  return (
    <>
      <div className="mx-3 border-t border-border/50">
        <div className="flex flex-wrap items-center gap-1">
          {collapsible ? (
            <button
              className="flex items-center gap-2 py-2.5 text-[12px] text-muted outline-none transition hover:text-ink focus:outline-none"
              onClick={() => setOpen((value) => !value)}
              ref={triggerRef}
              type="button"
            >
              <FolderOpen className="shrink-0" size={13} />
              <span className="max-w-[220px] truncate">
                {isProjectBound
                  ? `${activeProject.name} · ${describeProjectDirectory(activeProject.directory)}`
                  : t("chat:workInProject")}
              </span>
              <ChevronDown className={cn("shrink-0 transition-transform", open && "rotate-180")} size={13} />
            </button>
          ) : null}
        </div>
      </div>

      {collapsible && open ? (
        <div
          className="absolute left-0 right-0 top-[calc(100%-1px)] z-50 rounded-b-[1.5rem] border border-t-0 border-border/70 bg-card px-2 pb-2 pt-1 shadow-lg"
          ref={pickerRef}
        >
          {mode === "list" ? (
            <>
              <div className="relative mb-1 mt-1">
                <Search className="pointer-events-none absolute left-2.5 top-1/2 -translate-y-1/2 text-muted" size={12} />
                <input
                  autoFocus
                  className="form-control-focus w-full rounded-xl border border-border/50 bg-background/60 py-1.5 pl-7 pr-3 text-[12px] placeholder:text-muted"
                  onChange={(event) => setSearch(event.target.value)}
                  placeholder={t("chat:searchProjects")}
                  value={search}
                />
              </div>
              <div className="max-h-44 overflow-y-auto">
                {filteredProjects.map((project) => {
                  const unavailable = Boolean(project.status && project.status.state !== "available");
                  return (
                    <button
                      className={cn(
                        "flex w-full items-center gap-2 rounded-lg px-2.5 py-2 text-left text-[12px] outline-none transition hover:bg-hover focus:outline-none",
                        project.id === activeProjectId && selectionClasses.selected,
                        unavailable && "text-muted",
                      )}
                      key={project.id}
                      onClick={() => {
                        if (unavailable) return;
                        onProjectChange(project.id);
                        close();
                      }}
                      title={unavailable ? t("chat:projectDirectoryUnavailable") : undefined}
                      type="button"
                    >
                      <FolderOpen className="shrink-0 text-muted" size={12} />
                      <span className="truncate">{isUnboundProject(project.id) ? t("common:noProject") : project.name}</span>
                      {unavailable ? (
                        <TriangleAlert className="ml-auto shrink-0 text-danger" size={12} />
                      ) : !isUnboundProject(project.id) && project.directory ? (
                        <span className="ml-auto shrink-0 text-muted">{describeProjectDirectory(project.directory)}</span>
                      ) : null}
                    </button>
                  );
                })}
              </div>
              <button
                className="mt-0.5 flex w-full items-center gap-2 rounded-lg px-2.5 py-2 text-[12px] text-muted outline-none transition hover:bg-hover hover:text-ink focus:outline-none"
                onClick={() => setMode("add_menu")}
                type="button"
              >
                <FolderPlus size={12} />
                {t("chat:addNewProject")}
                <ChevronDown className="-rotate-90 ml-auto" size={12} />
              </button>
            </>
          ) : (
            <div className="py-1">
              <button className="mb-1 flex w-full items-center gap-2 rounded-lg px-2.5 py-2 text-left text-[12px] text-muted outline-none transition hover:bg-hover hover:text-ink focus:outline-none" onClick={() => setMode("list")} type="button">
                <ArrowLeft size={12} />
                {t("chat:backToProjects")}
              </button>
              <button className="flex w-full items-center gap-2 rounded-lg px-2.5 py-2 text-left text-[13px] text-ink outline-none transition hover:bg-hover focus:outline-none" onClick={() => { close(); setDirectoryDialogMode("create"); }} type="button">
                <Plus size={14} />
                {t("chat:startFromScratch")}
              </button>
              <button className="flex w-full items-center gap-2 rounded-lg px-2.5 py-2 text-left text-[13px] text-ink outline-none transition hover:bg-hover focus:outline-none" onClick={() => { close(); setDirectoryDialogMode("existing"); }} type="button">
                <FolderOpen size={14} />
                {t("chat:useExistingFolder")}
              </button>
            </div>
          )}
        </div>
      ) : null}

      {onCreateProject ? (
        <ProjectDirectoryDialog
          createDirectory={createFsDirectory}
          mode={directoryDialogMode}
          onClose={() => setDirectoryDialogMode(null)}
          onCreateProject={onCreateProject}
          readDirectory={readFsDirectory}
        />
      ) : null}
    </>
  );
}
