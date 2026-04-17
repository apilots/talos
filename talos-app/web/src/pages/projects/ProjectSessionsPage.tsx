import { useTranslation } from "react-i18next";
import { useNavigate, useParams } from "react-router-dom";
import { FolderSearch } from "lucide-react";

import { ProjectSessionRow, useProjectSessionsPageState } from "@/features/projects";
import { Button } from "@/shared/ui/Button";
import { Input } from "@/shared/ui/Input";
import { Panel } from "@/shared/ui/Panel";
import { SkeletonLine } from "@/shared/ui/SkeletonLine";

export function ProjectSessionsPage() {
  const { t } = useTranslation(["projects", "common"]);
  const navigate = useNavigate();
  const params = useParams();
  const projectId = params.projectId ?? "";
  const { notFound, project, query, sessionRows, sessionsQuery, setQuery } =
    useProjectSessionsPageState(projectId);
  const visibleCount = sessionRows.length;

  if (notFound) {
    return (
      <div className="h-full overflow-y-auto">
        <div className="mx-auto w-full max-w-[920px] px-8 py-8">
          <Panel className="p-6">
            <div className="text-base font-medium text-ink">{t("projects:projectNotFound")}</div>
            <div className="mt-2 text-sm text-muted">{t("projects:projectNotFoundDetail")}</div>
            <Button className="mt-4" onClick={() => navigate("/projects")} variant="secondary">
              {t("projects:backToProjects")}
            </Button>
          </Panel>
        </div>
      </div>
    );
  }

  return (
    <div className="h-full overflow-y-auto">
      <div className="mx-auto w-full max-w-[1040px] px-8 py-8">
        <div className="mb-5">
          <div className="flex items-center gap-2 text-xs">
            <button
              className="text-muted transition hover:text-ink"
              onClick={() => navigate("/projects")}
              type="button"
            >
              {t("projects:title")}
            </button>
            <span className="px-1 text-muted/40">/</span>
            <span className="truncate text-ink">{project?.name ?? <SkeletonLine className="h-4 w-28" />}</span>
          </div>
          <div className="mt-4 flex flex-wrap items-start justify-between gap-4">
            <div className="max-w-[640px]">
              <h1 className="font-display text-[30px] font-semibold tracking-[-0.05em] text-ink">
                {project?.name ?? ""}
              </h1>
              <p className="mt-3 text-sm leading-6 text-muted">{t("projects:sessionsDetail")}</p>
            </div>
            <div className="flex flex-wrap items-center gap-2">
              <div className="rounded-full border border-border/70 bg-background/55 px-3 py-1.5 text-[11px] uppercase tracking-[0.16em] text-muted">
                {t("projects:sessionsCount", { count: project?.sessionCount ?? visibleCount })}
              </div>
              {project?.directory ? (
                <div className="rounded-full border border-border/70 bg-background/55 px-3 py-1.5 text-xs text-muted">
                  {t("projects:directoryLabel")}: {project.directory}
                </div>
              ) : null}
            </div>
          </div>
        </div>

        <Panel className="overflow-hidden border-border/70 bg-card/94">
          <div className="border-b border-border/55 p-4">
            <div className="flex flex-wrap items-start justify-between gap-3">
              <div>
                <div className="text-[11px] uppercase tracking-[0.18em] text-muted">{t("projects:sessionsTitle")}</div>
                <div className="mt-1 text-sm text-muted">{t("projects:sessionsDetail")}</div>
              </div>
              <div className="rounded-full border border-border/70 bg-background/55 px-3 py-1.5 text-[11px] uppercase tracking-[0.16em] text-muted">
                {t("projects:visibleNow")}: {visibleCount}
              </div>
            </div>
            <div className="mt-4">
              <Input
                className="bg-background/72"
                onChange={(event) => setQuery(event.target.value)}
                placeholder={t("projects:searchSessions")}
                value={query}
              />
            </div>
          </div>

          {sessionsQuery.isLoading ? (
            <div className="p-4">
              <div className="space-y-3">
                {Array.from({ length: 6 }).map((_, index) => (
                  <SkeletonLine className="h-16 w-full" key={index} />
                ))}
              </div>
            </div>
          ) : null}

          {sessionsQuery.isError ? (
            <div className="px-4 py-6 text-sm text-muted">
              <div>{String(sessionsQuery.error ?? "Failed to load sessions.")}</div>
              <Button className="mt-3" onClick={() => void sessionsQuery.refetch()} variant="ghost">
                {t("common:retryNow")}
              </Button>
            </div>
          ) : null}

          {!sessionsQuery.isLoading && !sessionsQuery.isError && sessionRows.length > 0 ? (
            <div>
              {sessionRows.map((session) => (
                <ProjectSessionRow
                  key={session.id}
                  label={t("projects:sessionRowAria", { title: session.title })}
                  lastActivityAt={session.lastActivityAt}
                  model={session.model}
                  onSelect={() => navigate(`/sessions/${session.id}`)}
                  status={session.status}
                  title={session.title}
                />
              ))}
            </div>
          ) : null}

          {!sessionsQuery.isLoading && !sessionsQuery.isError && sessionRows.length === 0 ? (
            <div className="px-4 py-12">
              <div className="mx-auto flex max-w-[420px] flex-col items-center rounded-[calc(var(--radius-panel)-4px)] border border-dashed border-border/70 bg-background/32 px-6 py-10 text-center">
                <div className="flex size-12 items-center justify-center rounded-full border border-border/65 bg-card/86 text-muted">
                  <FolderSearch size={18} />
                </div>
                <div className="mt-4 text-sm font-medium text-ink">
                  {query.trim() ? t("projects:noMatchingSessions") : t("projects:noSessionsTitle")}
                </div>
                {!query.trim() ? (
                  <div className="mt-2 text-sm leading-6 text-muted">{t("projects:noSessionsDetail")}</div>
                ) : null}
              </div>
            </div>
          ) : null}
        </Panel>
      </div>
    </div>
  );
}
