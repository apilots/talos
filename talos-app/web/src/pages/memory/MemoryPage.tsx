import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { Save, Search } from "lucide-react";

import { useConnectionSettings } from "@/features/connection";
import { formatMemoryDate, useMemoryPage } from "@/features/memory";
import { MarkdownRenderer } from "@/widgets/message-list/MarkdownRenderer";
import { Button } from "@/shared/ui/Button";
import { Input, Textarea } from "@/shared/ui/Input";
import { Panel } from "@/shared/ui/Panel";

type MemoryViewMode = "preview" | "edit";

export function MemoryPage() {
  const { t } = useTranslation(["memory", "common"]);
  const { connection } = useConnectionSettings();
  const canEdit = connection.authStatus === "open" || connection.authStatus === "authenticated";
  const [viewMode, setViewMode] = useState<MemoryViewMode>("preview");
  const {
    query,
    setQuery,
    files,
    searchResults,
    searchResultDates,
    selectedDate,
    selectedDetail,
    editorContent,
    setEditorContent,
    selectDate,
    saveCurrent,
    isDirty,
    isLoadingList,
    isLoadingDetail,
    isSearching,
    isSearchPending,
    isSaving,
    searchError,
    saveError,
  } = useMemoryPage();

  useEffect(() => {
    if (!canEdit && viewMode !== "preview") {
      setViewMode("preview");
    }
  }, [canEdit, viewMode]);

  const showingSearch = query.trim().length > 0;
  const listDates = useMemo(
    () => (showingSearch ? searchResultDates : files.map((item) => item.date)),
    [files, searchResultDates, showingSearch],
  );

  const searchBusy = isSearchPending || isSearching;

  return (
    <div className="flex h-full w-full flex-col overflow-hidden">
      <div className="min-h-0 flex-1 overflow-y-auto">
        <div className="mx-auto flex w-full max-w-[1180px] flex-col px-8 py-8">
          <Panel className="mb-6 rounded-[var(--radius-panel)] border-border/75 bg-card/88 p-4">
            <div className="grid gap-4 xl:grid-cols-[minmax(0,1fr)_auto] xl:items-center">
              <div>
                <label className="block text-[11px] uppercase tracking-[0.22em] text-muted/80">
                  {t("common:search")}
                </label>
                <div className="relative mt-3">
                  <Search
                    className="pointer-events-none absolute left-3.5 top-1/2 -translate-y-1/2 text-muted/80"
                    size={14}
                  />
                  <Input
                    className="h-12 rounded-[var(--radius-control)] border-border/65 bg-background/82 pl-10"
                    onChange={(event) => setQuery(event.target.value)}
                    placeholder={t("memory:searchPlaceholder")}
                    value={query}
                  />
                </div>
                <div className="mt-3 text-xs text-muted">
                  {showingSearch
                    ? searchBusy
                      ? t("memory:searching")
                      : t("memory:searchResultsCount", { count: searchResults.length })
                    : t("memory:fileCount", { count: files.length })}
                </div>
              </div>
            </div>
          </Panel>

          <div className="grid gap-6 xl:grid-cols-[348px_minmax(0,1fr)]">
            <div className="space-y-4">
              <Panel className="rounded-[var(--radius-panel)] border-border/70 bg-card/78 p-2.5">
                <div className="mb-2 flex items-center justify-between px-2 pt-2 text-[11px] uppercase tracking-[0.22em] text-muted/80">
                  {showingSearch ? t("memory:matchingFiles") : t("memory:recentFiles")}
                  <span className="rounded-[var(--radius-control)] border border-border/65 bg-background/65 px-2 py-0.5 text-[10px] tracking-[0.16em]">
                    {listDates.length}
                  </span>
                </div>
                <div className="space-y-1">
                  {isLoadingList ? (
                    <div className="px-3 py-4 text-sm text-muted">{t("common:loading")}</div>
                  ) : listDates.length === 0 ? (
                    <div className="px-3 py-4 text-sm text-muted">{t("memory:noMemoryDetail")}</div>
                  ) : (
                    listDates.map((date) => (
                      <button
                        className={[
                          "w-full rounded-[var(--radius-control)] border px-3.5 py-3 text-left transition",
                          selectedDate === date
                            ? "border-border/80 bg-background/88 text-ink shadow-[0_10px_24px_rgba(95,84,64,0.045)]"
                            : "border-transparent text-muted hover:border-border/60 hover:bg-background/55",
                        ].join(" ")}
                        key={date}
                        onClick={() => selectDate(date)}
                        type="button"
                      >
                        <div className="font-medium">{date}</div>
                        <div className="mt-1 text-xs text-muted">{t("memory:dailyMemory")}</div>
                      </button>
                    ))
                  )}
                </div>
              </Panel>
            </div>

            <Panel className="rounded-[var(--radius-panel)] border-border/75 bg-card/90 p-5">
              {showingSearch ? (
                <div>
                  <div className="flex flex-wrap items-start justify-between gap-4">
                    <div>
                      <div className="text-xs uppercase tracking-[0.22em] text-muted">{t("common:search")}</div>
                      <h2 className="mt-2 font-display text-[24px] tracking-[-0.045em] text-ink">
                        {searchBusy ? t("memory:searching") : t("memory:matchingSnippets")}
                      </h2>
                      <p className="mt-2 text-sm text-muted">
                        {searchBusy
                          ? t("memory:detail")
                          : t("memory:searchResultsCount", { count: searchResults.length })}
                      </p>
                    </div>
                    <div className="rounded-[var(--radius-control)] border border-border/70 bg-background/60 px-3 py-2 text-xs text-muted">
                      {query.trim()}
                    </div>
                  </div>

                  <div className="mt-5 space-y-3">
                    {searchError ? (
                      <div className="rounded-[var(--radius-control)] border border-danger/20 bg-danger/5 px-3.5 py-3 text-sm text-danger">
                        {searchError}
                      </div>
                    ) : null}
                    {searchResults.length === 0 && !searchBusy ? (
                      <div className="rounded-[var(--radius-control)] border border-dashed border-border/65 bg-background/38 px-3.5 py-4 text-sm text-muted">
                        {t("memory:nothingMatched")}
                      </div>
                    ) : (
                      searchResults.map((result) => (
                        <button
                          className="w-full rounded-[var(--radius-control)] border border-border/65 bg-background/50 px-4 py-4 text-left transition hover:border-brand/30 hover:bg-background/74"
                          key={`${result.date}:${result.start_line}:${result.end_line}`}
                          onClick={() => selectDate(result.date)}
                          type="button"
                        >
                          <div className="flex flex-wrap items-start justify-between gap-3">
                            <div>
                              <div className="text-xs uppercase tracking-[0.18em] text-muted">
                                {result.date}
                              </div>
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
                      <div className="text-xs uppercase tracking-[0.22em] text-muted">{selectedDate}</div>
                      <h2 className="mt-2 font-display text-[24px] tracking-[-0.045em] text-ink">
                        {selectedDetail.version_token ? t("memory:editorTitle") : t("memory:newFileTitle")}
                      </h2>
                      <p className="mt-2 text-sm text-muted">
                        {selectedDetail.version_token
                          ? t("memory:lastUpdated", { date: formatMemoryDate(selectedDetail.updated_at) })
                          : t("memory:newFileDetail")}
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
                          <Button disabled={!isDirty || isSaving || isLoadingDetail} onClick={() => void saveCurrent()}>
                            <Save size={16} />
                            {isSaving ? t("memory:saving") : t("common:save")}
                          </Button>
                        ) : null}
                      </div>
                    ) : null}
                  </div>

                  {viewMode === "edit" && canEdit ? (
                    <Textarea
                      className="mt-4 min-h-[440px] rounded-[var(--radius-panel)] border-border/70 bg-[linear-gradient(180deg,rgba(255,255,255,0.32),rgba(255,255,255,0.16))] font-mono text-sm leading-6 shadow-[inset_0_1px_0_rgba(255,255,255,0.5)]"
                      onChange={(event) => setEditorContent(event.target.value)}
                      value={editorContent}
                    />
                  ) : (
                    <div className="mt-4 rounded-[var(--radius-panel)] border border-border/70 bg-background/36 p-5">
                      {selectedDetail.content.trim() ? (
                        <MarkdownRenderer className="markdown-body prose prose-sm max-w-none" text={selectedDetail.content} />
                      ) : (
                        <div className="text-sm text-muted">{t("memory:emptyPreview")}</div>
                      )}
                    </div>
                  )}

                  {saveError ? (
                    <div className="mt-3 rounded-[var(--radius-control)] border border-danger/20 bg-danger/5 px-3 py-2 text-sm text-danger">
                      {saveError}
                    </div>
                  ) : null}
                </div>
              )}
            </Panel>
          </div>
        </div>
      </div>
    </div>
  );
}
