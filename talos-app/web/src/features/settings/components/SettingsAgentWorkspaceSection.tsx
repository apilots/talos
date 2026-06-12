import { useEffect, useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Brain, Save, Search } from "lucide-react";
import { useTranslation } from "react-i18next";

import { useConnectionKey } from "@/features/chat";
import { useConnectionSettings } from "@/features/connection";
import { formatMemoryDate, useMemoryPage } from "@/features/memory";
import { formatWorkspaceDocDate, useWorkspaceDocsPage } from "@/features/workspace-docs";
import { getTalosClient } from "@/shared/api/client";
import { Button } from "@/shared/ui/Button";
import { Input, Textarea } from "@/shared/ui/Input";
import { Panel } from "@/shared/ui/Panel";
import {
  EditorModeSwitch,
  MarkdownPreview,
  type EditorMode,
} from "./AgentWorkspaceEditorPrimitives";
import { FieldBlock, SectionHeader } from "./SectionPrimitives";

type AgentWorkspaceMode = "general" | "instructions" | "memory";

export function SettingsAgentWorkspaceSection() {
  const { t } = useTranslation(["settings", "workspaceDocs", "memory", "common"]);
  const { connection } = useConnectionSettings();
  const connectionKey = useConnectionKey();
  const canEdit = connection.authStatus === "open" || connection.authStatus === "authenticated";
  const [mode, setMode] = useState<AgentWorkspaceMode>("general");
  const [docEditorMode, setDocEditorMode] = useState<EditorMode>("preview");
  const [memoryEditorMode, setMemoryEditorMode] = useState<EditorMode>("preview");
  const workspaceDocs = useWorkspaceDocsPage();
  const memory = useMemoryPage();
  const workspaceInfoQuery = useQuery({
    queryKey: ["talos", connectionKey, "workspace-info"],
    queryFn: () => getTalosClient().workspaceDocs.info(),
    refetchOnWindowFocus: false,
  });
  const baseWorkspacePath = workspaceInfoQuery.data?.base_workspace_dir ?? "";
  const userWorkspacePath = workspaceInfoQuery.data?.user_workspace_dir ?? "";

  useEffect(() => {
    if (!canEdit) {
      setDocEditorMode("preview");
      setMemoryEditorMode("preview");
    }
  }, [canEdit]);

  const showingMemorySearch = memory.query.trim().length > 0;
  const memoryDates = useMemo(
    () => (showingMemorySearch ? memory.searchResultDates : memory.files.map((item) => item.date)),
    [memory.files, memory.searchResultDates, showingMemorySearch],
  );
  const memoryBusy = memory.isSearchPending || memory.isSearching;

  return (
    <div className="space-y-5">
      <Panel className="rounded-[var(--radius-panel)] border-border/75 bg-card/88 p-6">
        <SectionHeader
          icon={<Brain size={18} />}
          subtitle={t("settings:agentWorkspaceDetail")}
          title={t("settings:agentWorkspaceTitle")}
        />
        <div className="mt-5 flex flex-wrap gap-2 rounded-[var(--radius-control)] border border-border/70 bg-background/48 p-1">
          <button
            className={[
              "rounded-[var(--radius-control)] px-3 py-2 text-sm transition",
              mode === "general" ? "bg-card text-ink shadow-panel" : "text-muted hover:text-ink",
            ].join(" ")}
            onClick={() => setMode("general")}
            type="button"
          >
            {t("settings:agentGeneral")}
          </button>
          <button
            className={[
              "rounded-[var(--radius-control)] px-3 py-2 text-sm transition",
              mode === "instructions" ? "bg-card text-ink shadow-panel" : "text-muted hover:text-ink",
            ].join(" ")}
            onClick={() => setMode("instructions")}
            type="button"
          >
            {t("settings:agentInstructions")}
          </button>
          <button
            className={[
              "rounded-[var(--radius-control)] px-3 py-2 text-sm transition",
              mode === "memory" ? "bg-card text-ink shadow-panel" : "text-muted hover:text-ink",
            ].join(" ")}
            onClick={() => setMode("memory")}
            type="button"
          >
            {t("settings:agentMemory")}
          </button>
        </div>
      </Panel>

      {mode === "general" ? (
        <Panel className="rounded-[var(--radius-panel)] border-border/75 bg-card/90 p-5">
          <div className="flex flex-wrap items-start justify-between gap-4">
            <div>
              <div className="text-xs uppercase tracking-[0.22em] text-muted">
                {t("settings:agentGeneral")}
              </div>
              <h2 className="mt-2 font-display text-[24px] tracking-[-0.045em] text-ink">
                {t("settings:workspacePathValue")}
              </h2>
              <p className="mt-2 text-sm text-muted">{t("settings:agentWorkspacePathDetail")}</p>
            </div>
          </div>
          <div className="mt-5 max-w-3xl">
            <FieldBlock label={t("settings:baseWorkspace")}>
              <Input disabled value={baseWorkspacePath || t("common:loading")} />
            </FieldBlock>
            <div className="mt-4">
              <FieldBlock label={t("settings:currentUserWorkspace")}>
                <Input disabled value={userWorkspacePath || t("common:loading")} />
              </FieldBlock>
            </div>
            {workspaceInfoQuery.error instanceof Error ? (
              <div className="mt-4 rounded-[var(--radius-control)] border border-danger/20 bg-danger/5 px-3 py-2 text-sm text-danger">
                {workspaceInfoQuery.error.message}
              </div>
            ) : null}
          </div>
        </Panel>
      ) : null}

      {mode === "instructions" ? (
        <div className="grid gap-5 xl:grid-cols-[260px_minmax(0,1fr)]">
          <Panel className="rounded-[var(--radius-panel)] border-border/70 bg-card/78 p-2.5">
            <div className="mb-2 px-2 pt-2 text-[11px] uppercase tracking-[0.22em] text-muted/80">
              {t("workspaceDocs:documents")}
            </div>
            <div className="space-y-1">
              {workspaceDocs.docKinds.map((kind) => (
                <button
                  className={[
                    "w-full rounded-[var(--radius-control)] border px-3.5 py-3 text-left transition",
                    workspaceDocs.selectedDocKind === kind
                      ? "border-border/80 bg-background/88 text-ink shadow-[0_10px_24px_rgba(95,84,64,0.045)]"
                      : "border-transparent text-muted hover:border-border/60 hover:bg-background/55",
                  ].join(" ")}
                  key={kind}
                  onClick={() => workspaceDocs.setSelectedDocKind(kind)}
                  type="button"
                >
                  <div className="font-medium">{workspaceDocs.docLabels[kind]}</div>
                  <div className="mt-1 text-xs text-muted">{kind}</div>
                </button>
              ))}
            </div>
          </Panel>

          <Panel className="rounded-[var(--radius-panel)] border-border/75 bg-card/90 p-5">
            <div className="flex flex-wrap items-start justify-between gap-4">
              <div>
                <div className="text-xs uppercase tracking-[0.22em] text-muted">{t("settings:agentInstructions")}</div>
                <h2 className="mt-2 font-display text-[24px] tracking-[-0.045em] text-ink">
                  {workspaceDocs.docLabels[workspaceDocs.selectedDocKind]}
                </h2>
                <p className="mt-2 text-sm text-muted">
                  {workspaceDocs.selectedDetail.exists
                    ? t("workspaceDocs:lastUpdated", {
                        date: formatWorkspaceDocDate(workspaceDocs.selectedDetail.updated_at),
                      })
                    : t("workspaceDocs:missingDetail")}
                </p>
              </div>
              {canEdit ? (
                <div className="flex items-center gap-3">
                  <EditorModeSwitch mode={docEditorMode} setMode={setDocEditorMode} />
                  {docEditorMode === "edit" ? (
                    <Button
                      disabled={!workspaceDocs.isDirty || workspaceDocs.isSaving || workspaceDocs.isLoading}
                      onClick={() => void workspaceDocs.saveCurrent()}
                      type="button"
                    >
                      <Save size={16} />
                      {workspaceDocs.isSaving ? t("workspaceDocs:saving") : t("common:save")}
                    </Button>
                  ) : null}
                </div>
              ) : null}
            </div>

            <div className="mt-4 rounded-[var(--radius-control)] border border-border/70 bg-background/52 px-3 py-2 text-xs text-muted">
              {workspaceDocs.selectedDetail.path}
            </div>
            {workspaceDocs.loadError ? (
              <div className="mt-4 rounded-[var(--radius-control)] border border-danger/20 bg-danger/5 px-3 py-2 text-sm text-danger">
                {workspaceDocs.loadError}
              </div>
            ) : null}
            {docEditorMode === "edit" && canEdit ? (
              <Textarea
                className="mt-4 min-h-[440px] rounded-[var(--radius-panel)] border-border/70 bg-[linear-gradient(180deg,rgba(255,255,255,0.32),rgba(255,255,255,0.16))] font-mono text-sm leading-6 shadow-[inset_0_1px_0_rgba(255,255,255,0.5)]"
                onChange={(event) => workspaceDocs.setDocDraft(event.target.value)}
                value={workspaceDocs.docDraft}
              />
            ) : (
              <MarkdownPreview emptyText={t("workspaceDocs:emptyPreview")} text={workspaceDocs.selectedDetail.content} />
            )}
            {workspaceDocs.saveError ? (
              <div className="mt-3 rounded-[var(--radius-control)] border border-danger/20 bg-danger/5 px-3 py-2 text-sm text-danger">
                {workspaceDocs.saveError}
              </div>
            ) : null}
          </Panel>
        </div>
      ) : null}

      {mode === "memory" ? (
        <div className="grid gap-5 xl:grid-cols-[300px_minmax(0,1fr)]">
          <Panel className="rounded-[var(--radius-panel)] border-border/70 bg-card/78 p-2.5">
            <div className="px-2 pt-2">
              <div className="text-[11px] uppercase tracking-[0.22em] text-muted/80">{t("common:search")}</div>
              <div className="relative mt-3">
                <Search
                  className="pointer-events-none absolute left-3.5 top-1/2 -translate-y-1/2 text-muted/80"
                  size={14}
                />
                <Input
                  className="h-11 rounded-[var(--radius-control)] border-border/65 bg-background/82 pl-10"
                  onChange={(event) => memory.setQuery(event.target.value)}
                  placeholder={t("memory:searchPlaceholder")}
                  value={memory.query}
                />
              </div>
              <div className="mt-3 text-xs text-muted">
                {showingMemorySearch
                  ? memoryBusy
                    ? t("memory:searching")
                    : t("memory:searchResultsCount", { count: memory.searchResults.length })
                  : t("memory:fileCount", { count: memory.files.length })}
              </div>
            </div>
            <div className="mt-4 space-y-1">
              {memory.isLoadingList ? (
                <div className="px-3 py-4 text-sm text-muted">{t("common:loading")}</div>
              ) : memoryDates.length === 0 ? (
                <div className="px-3 py-4 text-sm text-muted">{t("memory:noMemoryDetail")}</div>
              ) : (
                memoryDates.map((date) => (
                  <button
                    className={[
                      "w-full rounded-[var(--radius-control)] border px-3.5 py-3 text-left transition",
                      memory.selectedDate === date
                        ? "border-border/80 bg-background/88 text-ink shadow-[0_10px_24px_rgba(95,84,64,0.045)]"
                        : "border-transparent text-muted hover:border-border/60 hover:bg-background/55",
                    ].join(" ")}
                    key={date}
                    onClick={() => memory.selectDate(date)}
                    type="button"
                  >
                    <div className="font-medium">{date}</div>
                    <div className="mt-1 text-xs text-muted">{`memory/${date}.md`}</div>
                  </button>
                ))
              )}
            </div>
          </Panel>

          <Panel className="rounded-[var(--radius-panel)] border-border/75 bg-card/90 p-5">
            {showingMemorySearch ? (
              <div>
                <div className="flex flex-wrap items-start justify-between gap-4">
                  <div>
                    <div className="text-xs uppercase tracking-[0.22em] text-muted">{t("common:search")}</div>
                    <h2 className="mt-2 font-display text-[24px] tracking-[-0.045em] text-ink">
                      {memoryBusy ? t("memory:searching") : t("memory:matchingSnippets")}
                    </h2>
                    <p className="mt-2 text-sm text-muted">
                      {memoryBusy ? t("memory:detail") : t("memory:searchResultsCount", { count: memory.searchResults.length })}
                    </p>
                  </div>
                </div>
                <div className="mt-5 space-y-3">
                  {memory.searchError ? (
                    <div className="rounded-[var(--radius-control)] border border-danger/20 bg-danger/5 px-3.5 py-3 text-sm text-danger">
                      {memory.searchError}
                    </div>
                  ) : null}
                  {memory.searchResults.length === 0 && !memoryBusy ? (
                    <div className="rounded-[var(--radius-control)] border border-dashed border-border/65 bg-background/38 px-3.5 py-4 text-sm text-muted">
                      {t("memory:nothingMatched")}
                    </div>
                  ) : (
                    memory.searchResults.map((result) => (
                      <button
                        className="w-full rounded-[var(--radius-control)] border border-border/65 bg-background/50 px-4 py-4 text-left transition hover:border-brand/30 hover:bg-background/74"
                        key={`${result.path}:${result.start_line}:${result.end_line}`}
                        onClick={() => memory.selectDate(result.path.split("/").at(-1)?.replace(/\.md$/, "") ?? memory.selectedDate)}
                        type="button"
                      >
                        <div className="flex flex-wrap items-start justify-between gap-3">
                          <div>
                            <div className="text-xs uppercase tracking-[0.18em] text-muted">
                              {result.path.split("/").at(-1)?.replace(/\.md$/, "")}
                            </div>
                            <div className="mt-1 text-xs text-muted">{result.path}</div>
                          </div>
                          <div className="rounded-[var(--radius-control)] border border-border/65 bg-background/68 px-2.5 py-1 text-[11px] text-muted">
                            {t("memory:lineRange", { start: result.start_line, end: result.end_line })}
                          </div>
                        </div>
                        <div className="mt-3 text-sm leading-6 text-ink">{result.snippet}</div>
                      </button>
                    ))
                  )}
                </div>
              </div>
            ) : (
              <div>
                <div className="flex flex-wrap items-start justify-between gap-4">
                  <div>
                    <div className="text-xs uppercase tracking-[0.22em] text-muted">{memory.selectedDate}</div>
                    <h2 className="mt-2 font-display text-[24px] tracking-[-0.045em] text-ink">
                      {memory.selectedDetail.version_token ? t("memory:editorTitle") : t("memory:newFileTitle")}
                    </h2>
                    <p className="mt-2 text-sm text-muted">
                      {memory.selectedDetail.version_token
                        ? t("memory:lastUpdated", { date: formatMemoryDate(memory.selectedDetail.updated_at) })
                        : t("memory:newFileDetail")}
                    </p>
                  </div>
                  {canEdit ? (
                    <div className="flex items-center gap-3">
                      <EditorModeSwitch mode={memoryEditorMode} setMode={setMemoryEditorMode} />
                      {memoryEditorMode === "edit" ? (
                        <Button
                          disabled={!memory.isDirty || memory.isSaving || memory.isLoadingDetail}
                          onClick={() => void memory.saveCurrent()}
                          type="button"
                        >
                          <Save size={16} />
                          {memory.isSaving ? t("memory:saving") : t("common:save")}
                        </Button>
                      ) : null}
                    </div>
                  ) : null}
                </div>
                <div className="mt-4 rounded-[var(--radius-control)] border border-border/70 bg-background/52 px-3 py-2 text-xs text-muted">
                  {memory.selectedDetail.path}
                </div>
                {memoryEditorMode === "edit" && canEdit ? (
                  <Textarea
                    className="mt-4 min-h-[440px] rounded-[var(--radius-panel)] border-border/70 bg-[linear-gradient(180deg,rgba(255,255,255,0.32),rgba(255,255,255,0.16))] font-mono text-sm leading-6 shadow-[inset_0_1px_0_rgba(255,255,255,0.5)]"
                    onChange={(event) => memory.setEditorContent(event.target.value)}
                    value={memory.editorContent}
                  />
                ) : (
                  <MarkdownPreview emptyText={t("memory:emptyPreview")} text={memory.selectedDetail.content} />
                )}
                {memory.saveError ? (
                  <div className="mt-3 rounded-[var(--radius-control)] border border-danger/20 bg-danger/5 px-3 py-2 text-sm text-danger">
                    {memory.saveError}
                  </div>
                ) : null}
              </div>
            )}
          </Panel>
        </div>
      ) : null}
    </div>
  );
}
