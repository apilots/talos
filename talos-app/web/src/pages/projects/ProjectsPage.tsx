import { FolderPlus, X } from "lucide-react";
import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router-dom";

import { useCreateProjectMutation, useDeleteProjectMutation } from "@/features/chat";
import { ProjectRow, useProjectsPageState, type ProjectSort } from "@/features/projects";
import { cn } from "@/shared/lib/cn";
import { Button } from "@/shared/ui/Button";
import { Input } from "@/shared/ui/Input";
import { Panel } from "@/shared/ui/Panel";
import { SkeletonLine } from "@/shared/ui/SkeletonLine";

function deriveProjectName(directory: string) {
  const normalized = directory.trim().replace(/\/+$/, "");
  return normalized.split("/").filter(Boolean).at(-1) || "";
}

function parseProjectDeleteError(error: unknown, t: (key: string, options?: Record<string, unknown>) => string) {
  if (error instanceof Error && error.message === "PROJECT_DELETE_CONFLICT") {
    return t("projects:deleteProjectBlocked");
  }
  return String(error);
}

export function ProjectsPage() {
  const { t } = useTranslation(["projects", "common"]);
  const navigate = useNavigate();
  const createProject = useCreateProjectMutation();
  const deleteProject = useDeleteProjectMutation();
  const { filteredProjects, projectsQuery, query, setQuery, setSort, sort } = useProjectsPageState();
  const [createOpen, setCreateOpen] = useState(false);
  const [path, setPath] = useState("");
  const [name, setName] = useState("");
  const [nameTouched, setNameTouched] = useState(false);
  const [formError, setFormError] = useState<string | null>(null);
  const [deleteError, setDeleteError] = useState<string | null>(null);

  const isCreating = createProject.isPending;
  const showEmpty = !projectsQuery.isLoading && !projectsQuery.isError && filteredProjects.length === 0;
  const emptyTitle = useMemo(() => {
    if (query.trim()) {
      return t("projects:noMatchingProjects");
    }
    return t("projects:noProjectsTitle");
  }, [query, t]);
  const emptyDetail = useMemo(() => {
    if (query.trim()) {
      return undefined;
    }
    return t("projects:noProjectsDetail");
  }, [query, t]);

  function closeCreateDialog() {
    setCreateOpen(false);
    setPath("");
    setName("");
    setNameTouched(false);
    setFormError(null);
  }

  async function handleDeleteProject(projectId: string, projectName: string) {
    if (!window.confirm(t("projects:deleteProjectConfirm", { name: projectName }))) {
      return;
    }
    setDeleteError(null);
    try {
      await deleteProject.mutateAsync(projectId);
    } catch (error) {
      setDeleteError(parseProjectDeleteError(error, t));
    }
  }

  async function handleCreateProject() {
    const normalizedPath = path.trim();
    const normalizedName = name.trim();
    if (!normalizedName) {
      setFormError(t("projects:invalidName"));
      return;
    }
    if (!normalizedPath.startsWith("/")) {
      setFormError(t("projects:invalidPath"));
      return;
    }
    setFormError(null);
    try {
      const project = await createProject.mutateAsync({
        directory: normalizedPath,
        name: normalizedName,
      });
      closeCreateDialog();
      navigate(`/projects/${project.id}`);
    } catch (error) {
      setFormError(String(error));
    }
  }

  const sortOptions: Array<{ value: ProjectSort; label: string }> = [
    { value: "activity", label: t("projects:sortActivity") },
    { value: "last_edited", label: t("projects:sortLastEdited") },
    { value: "date_created", label: t("projects:sortDateCreated") },
  ];

  return (
    <div className="h-full overflow-y-auto">
      <div className="mx-auto w-full max-w-[980px] px-8 py-8">
        <div className="flex flex-wrap items-center justify-between gap-4">
          <h1 className="font-display text-[31px] font-semibold tracking-[-0.05em] text-ink">{t("projects:title")}</h1>
          <Button className="shadow-sm" onClick={() => setCreateOpen(true)}>
            <FolderPlus size={16} />
            {t("projects:newProject")}
          </Button>
        </div>

        <Panel className="mt-7 overflow-hidden border-border/70 bg-card/94">
          <div className="border-b border-border/55 p-5">
            <Input
              className="h-12 bg-background/66"
              onChange={(event) => {
                setDeleteError(null);
                setQuery(event.target.value);
              }}
              placeholder={t("projects:searchProjects")}
              value={query}
            />
            <div className="mt-4 flex items-center justify-end gap-2.5">
              <span className="text-sm text-muted/90">{t("projects:sortBy")}</span>
              <select
                className="min-w-[156px] rounded-[var(--radius-control)] border border-border/70 bg-background/60 px-3 py-2 text-sm text-ink outline-none transition hover:border-brand/30 focus:border-brand/45"
                onChange={(event) => setSort(event.target.value as ProjectSort)}
                value={sort}
              >
                {sortOptions.map((option) => (
                  <option key={option.value} value={option.value}>
                    {option.label}
                  </option>
                ))}
              </select>
            </div>
            {deleteError ? (
              <div className="mt-4 rounded-[var(--radius-control)] border border-danger/20 bg-danger/5 px-3 py-2 text-sm text-danger">
                {deleteError}
              </div>
            ) : null}
          </div>

          {projectsQuery.isLoading ? (
            <div className="grid gap-4 p-5 md:grid-cols-2">
              {Array.from({ length: 4 }).map((_, index) => (
                <SkeletonLine className="h-[164px] w-full rounded-[var(--radius-panel)]" key={index} />
              ))}
            </div>
          ) : null}

          {projectsQuery.isError ? (
            <div className="px-4 py-6 text-sm text-muted">
              <div>{String(projectsQuery.error ?? "Failed to load projects.")}</div>
              <Button className="mt-3" onClick={() => void projectsQuery.refetch()} variant="ghost">
                {t("common:retryNow")}
              </Button>
            </div>
          ) : null}

          {!projectsQuery.isLoading && !projectsQuery.isError && filteredProjects.length > 0 ? (
            <div className="grid gap-4 p-5 md:grid-cols-2">
              {filteredProjects.map((project) => (
                <ProjectRow
                  deleteDisabled={(project.sessionCount ?? 0) > 0}
                  deleteDisabledReason={t("projects:deleteProjectDisabledHint")}
                  deleteLabel={
                    deleteProject.isPending && deleteProject.variables === project.id
                      ? t("projects:deletingProject")
                      : t("common:delete")
                  }
                  deleting={deleteProject.isPending && deleteProject.variables === project.id}
                  key={project.id}
                  label={t("projects:projectRowAria", { name: project.name })}
                  onDelete={() => void handleDeleteProject(project.id, project.name)}
                  onSelect={() => navigate(`/projects/${project.id}`)}
                  project={project}
                />
              ))}
            </div>
          ) : null}

          {showEmpty ? (
            <div className="px-5 py-12">
              <div className="text-sm font-medium text-ink">{emptyTitle}</div>
              {emptyDetail ? <div className="mt-2 text-sm text-muted">{emptyDetail}</div> : null}
            </div>
          ) : null}
        </Panel>
      </div>

      {createOpen ? (
        <div className="fixed inset-0 z-50 flex items-start justify-center bg-background/44 px-4 pt-24 backdrop-blur-[10px]">
          <div className="absolute inset-0" onClick={closeCreateDialog} role="presentation" />
          <div className="relative z-10 w-[min(436px,calc(100vw-32px))] rounded-[calc(var(--radius-panel)+2px)] border border-border/75 bg-card/97 p-5 shadow-[0_24px_80px_rgba(0,0,0,0.26)] backdrop-blur-[18px]">
            <div className="flex items-start justify-between gap-4">
              <div>
                <div className="text-[11px] uppercase tracking-[0.22em] text-brand/75">{t("projects:title")}</div>
                <h2 className="mt-2 font-display text-[24px] font-semibold tracking-[-0.04em] text-ink">{t("projects:newProject")}</h2>
                <div className="mt-2 text-sm text-muted">{t("projects:pathHint")}</div>
              </div>
              <button
                className="flex size-9 items-center justify-center rounded-full border border-border/75 bg-background/52 text-muted transition hover:border-brand/30 hover:text-ink"
                onClick={closeCreateDialog}
                type="button"
              >
                <X size={16} />
              </button>
            </div>

            <div className="mt-5 space-y-4">
              <div>
                <div className="mb-2 text-sm font-medium text-ink">{t("projects:projectNameLabel")}</div>
                <Input
                  className="bg-background/66"
                  onChange={(event) => {
                    setNameTouched(true);
                    setName(event.target.value);
                  }}
                  placeholder={t("projects:projectNamePlaceholder")}
                  value={name}
                />
              </div>

              <div>
                <div className="mb-2 text-sm font-medium text-ink">{t("projects:pathLabel")}</div>
                <Input
                  className="bg-background/66"
                  onChange={(event) => {
                    const nextPath = event.target.value;
                    setPath(nextPath);
                    if (!nameTouched) {
                      setName(deriveProjectName(nextPath));
                    }
                  }}
                  onKeyDown={(event) => {
                    if (event.key === "Enter") {
                      void handleCreateProject();
                    }
                  }}
                  placeholder={t("projects:pathPlaceholder")}
                  value={path}
                />
                <div className={cn("mt-2 text-xs", formError ? "text-danger" : "text-muted")}>
                  {formError ?? t("projects:pathHint")}
                </div>
              </div>

              <div className="flex items-center justify-end gap-3 pt-1">
                <Button onClick={closeCreateDialog} variant="ghost">
                  {t("projects:cancelCreate")}
                </Button>
                <Button disabled={isCreating} onClick={() => void handleCreateProject()}>
                  {t("projects:createProject")}
                </Button>
              </div>
            </div>
          </div>
        </div>
      ) : null}
    </div>
  );
}
