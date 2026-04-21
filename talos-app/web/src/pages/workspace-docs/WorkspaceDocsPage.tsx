import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { useConnectionSettings } from "@/features/connection";
import { formatWorkspaceDocDate, useWorkspaceDocsPage } from "@/features/workspace-docs";
import { MarkdownRenderer } from "@/widgets/message-list/MarkdownRenderer";
import { Button } from "@/shared/ui/Button";
import { Textarea } from "@/shared/ui/Input";
import { Panel } from "@/shared/ui/Panel";

type WorkspaceDocViewMode = "preview" | "edit";

export function WorkspaceDocsPage() {
  const { t } = useTranslation(["workspaceDocs", "common"]);
  const { connection } = useConnectionSettings();
  const canEdit = connection.authRole === "admin";
  const [viewMode, setViewMode] = useState<WorkspaceDocViewMode>("preview");
  const {
    docKinds,
    docLabels,
    selectedDocKind,
    setSelectedDocKind,
    selectedDetail,
    docDraft,
    setDocDraft,
    saveCurrent,
    isDirty,
    isLoading,
    isSaving,
    loadError,
    saveError,
  } = useWorkspaceDocsPage();

  useEffect(() => {
    if (!canEdit && viewMode !== "preview") {
      setViewMode("preview");
    }
  }, [canEdit, viewMode]);

  return (
    <div className="flex h-full w-full flex-col overflow-hidden">
      <div className="min-h-0 flex-1 overflow-y-auto">
        <div className="mx-auto flex w-full max-w-[1180px] flex-col gap-6 px-8 py-8">
          <div className="grid gap-6 xl:grid-cols-[260px_minmax(0,1fr)]">
            <Panel className="rounded-[var(--radius-panel)] border-border/70 bg-card/78 p-2.5">
              <div className="mb-2 px-2 pt-2 text-[11px] uppercase tracking-[0.22em] text-muted/80">
                {t("documents")}
              </div>
              <div className="space-y-1">
                {docKinds.map((kind) => (
                  <button
                    className={[
                      "w-full rounded-[var(--radius-control)] border px-3.5 py-3 text-left transition",
                      selectedDocKind === kind
                        ? "border-border/80 bg-background/88 text-ink shadow-[0_10px_24px_rgba(95,84,64,0.045)]"
                        : "border-transparent text-muted hover:border-border/60 hover:bg-background/55",
                    ].join(" ")}
                    key={kind}
                    onClick={() => setSelectedDocKind(kind)}
                    type="button"
                  >
                    <div className="font-medium">{docLabels[kind]}</div>
                    <div className="mt-1 text-xs text-muted">{kind}</div>
                  </button>
                ))}
              </div>
            </Panel>

            <Panel className="rounded-[var(--radius-panel)] border-border/75 bg-card/90 p-5">
              <div className="flex flex-wrap items-start justify-between gap-4">
                <div>
                  <div className="text-xs uppercase tracking-[0.22em] text-muted">{t("pageLabel")}</div>
                  <h1 className="mt-2 font-display text-[24px] tracking-[-0.045em] text-ink">
                    {docLabels[selectedDocKind]}
                  </h1>
                  <p className="mt-2 text-sm text-muted">
                    {selectedDetail.exists
                      ? t("lastUpdated", { date: formatWorkspaceDocDate(selectedDetail.updated_at) })
                      : t("missingDetail")}
                  </p>
                </div>
                {canEdit ? (
                  <div className="flex items-center gap-3">
                    <div className="flex items-center gap-2 rounded-[var(--radius-control)] border border-border/70 bg-background/48 p-1">
                      <button
                        className={[
                          "rounded-[var(--radius-control)] px-3 py-2 text-sm transition",
                          viewMode === "preview" ? "bg-card text-ink shadow-panel" : "text-muted hover:text-ink",
                        ].join(" ")}
                        onClick={() => setViewMode("preview")}
                        type="button"
                      >
                        {t("common:preview")}
                      </button>
                      <button
                        className={[
                          "rounded-[var(--radius-control)] px-3 py-2 text-sm transition",
                          viewMode === "edit" ? "bg-card text-ink shadow-panel" : "text-muted hover:text-ink",
                        ].join(" ")}
                        onClick={() => setViewMode("edit")}
                        type="button"
                      >
                        {t("common:edit")}
                      </button>
                    </div>
                    {viewMode === "edit" ? (
                      <Button disabled={!isDirty || isSaving || isLoading} onClick={() => void saveCurrent()}>
                        {isSaving ? t("saving") : t("common:save")}
                      </Button>
                    ) : null}
                  </div>
                ) : null}
              </div>

              <div className="mt-4 rounded-[var(--radius-control)] border border-border/70 bg-background/52 px-3 py-2 text-xs text-muted">
                {selectedDetail.path}
              </div>

              {loadError ? (
                <div className="mt-4 rounded-[var(--radius-control)] border border-danger/20 bg-danger/5 px-3 py-2 text-sm text-danger">
                  {loadError}
                </div>
              ) : null}

              {viewMode === "edit" && canEdit ? (
                <Textarea
                  className="mt-4 min-h-[440px] rounded-[var(--radius-panel)] border-border/70 bg-[linear-gradient(180deg,rgba(255,255,255,0.32),rgba(255,255,255,0.16))] font-mono text-sm leading-6 shadow-[inset_0_1px_0_rgba(255,255,255,0.5)]"
                  onChange={(event) => setDocDraft(event.target.value)}
                  value={docDraft}
                />
              ) : (
                <div className="mt-4 rounded-[var(--radius-panel)] border border-border/70 bg-background/36 p-5">
                  {selectedDetail.content.trim() ? (
                    <MarkdownRenderer className="markdown-body prose prose-sm max-w-none" text={selectedDetail.content} />
                  ) : (
                    <div className="text-sm text-muted">{t("emptyPreview")}</div>
                  )}
                </div>
              )}

              {saveError ? (
                <div className="mt-3 rounded-[var(--radius-control)] border border-danger/20 bg-danger/5 px-3 py-2 text-sm text-danger">
                  {saveError}
                </div>
              ) : null}
            </Panel>
          </div>
        </div>
      </div>
    </div>
  );
}
