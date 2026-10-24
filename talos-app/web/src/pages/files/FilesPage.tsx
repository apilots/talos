import { selectionClasses } from "@/shared/ui/controlStyles";
import { useEffect, useMemo, useRef } from "react";
import { useTranslation } from "react-i18next";
import { useLocation } from "react-router-dom";
import { FileCode2, FileImage, FileSpreadsheet, FileText, Grid2x2, List, Plus } from "lucide-react";

import {
  classifyFile,
  FileDetailsPanel,
  formatFileMeta,
  formatFileSize,
  useFilesPage,
  type FileCategory,
} from "@/features/files";
import { Input } from "@/shared/ui/Input";
import { Panel } from "@/shared/ui/Panel";

import "./FilesPage.css";

function iconForFile(category: FileCategory) {
  switch (category) {
    case "code":
      return <FileCode2 size={18} />;
    case "image":
      return <FileImage size={18} />;
    case "data":
      return <FileSpreadsheet size={18} />;
    default:
      return <FileText size={18} />;
  }
}

export function FilesPage() {
  const { i18n, t } = useTranslation(["files", "common"]);
  const location = useLocation();
  const navigationKeyRef = useRef(location.key);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const files = useFilesPage();
  const locale = i18n.resolvedLanguage ?? i18n.language;
  const categoryLabels = useMemo<Record<FileCategory, string>>(
    () => ({
      all: t("files:categories.all"),
      document: t("files:categories.document"),
      code: t("files:categories.code"),
      image: t("files:categories.image"),
      data: t("files:categories.data"),
    }),
    [t],
  );

  useEffect(() => {
    if (navigationKeyRef.current === location.key) {
      return;
    }
    navigationKeyRef.current = location.key;
    files.showAllFiles();
  }, [files.showAllFiles, location.key]);

  return (
    <div className="flex h-full w-full flex-col overflow-hidden">
      <div className="min-h-0 flex-1 overflow-y-auto">
        <div className="files-page-content mx-auto flex w-full max-w-[1120px] flex-col gap-5 px-8 py-7">
          <Panel className="rounded-[var(--radius-panel)] border-border/80 bg-card/[0.82] px-5 py-4">
            <div className="flex flex-wrap items-center justify-between gap-3">
              <Input
                className="min-w-0 flex-1"
                onChange={(event) => files.setQuery(event.target.value)}
                placeholder={t("files:searchPlaceholder")}
                value={files.query}
              />
              <input
                className="hidden"
                onChange={(event) => {
                  const file = event.target.files?.[0];
                  event.target.value = "";
                  if (file) {
                    void files.uploadFile(file).catch(() => undefined);
                  }
                }}
                ref={fileInputRef}
                type="file"
              />
              <div className="flex items-center gap-1.5">
                <button
                  aria-label={t("files:listView")}
                  className={files.view === "list" ? `rounded-xl border p-2 ${selectionClasses.border} ${selectionClasses.selected}` : `rounded-xl border border-border bg-card p-2 ${selectionClasses.idle}`}
                  onClick={() => files.setView("list")}
                  type="button"
                >
                  <List size={15} />
                </button>
                <button
                  aria-label={t("files:gridView")}
                  className={files.view === "grid" ? `rounded-xl border p-2 ${selectionClasses.border} ${selectionClasses.selected}` : `rounded-xl border border-border bg-card p-2 ${selectionClasses.idle}`}
                  onClick={() => files.setView("grid")}
                  type="button"
                >
                  <Grid2x2 size={15} />
                </button>
              </div>
            </div>
            <div className="mt-3 flex flex-wrap items-center gap-1.5">
              {(Object.keys(categoryLabels) as FileCategory[]).map((itemCategory) => (
                <button
                  className={files.category === itemCategory ? `rounded-full border px-3 py-1.5 text-xs ${selectionClasses.border} ${selectionClasses.selected}` : `rounded-full border border-border bg-card px-3 py-1.5 text-xs ${selectionClasses.idle}`}
                  key={itemCategory}
                  onClick={() => {
                    if (itemCategory === "all") {
                      files.showAllFiles();
                      return;
                    }
                    files.setCategory(itemCategory);
                  }}
                  type="button"
                >
                  {categoryLabels[itemCategory]} ({files.hasMore ? "≥" : ""}
                  {files.categorized[itemCategory]})
                </button>
              ))}
            </div>
            {files.hasMore ? (
              <p className="mt-2 text-xs text-muted">
                {t("files:loadedFilesNotice", { count: files.items.length })}
              </p>
            ) : null}
          </Panel>

          {files.error ? (
            <Panel className="rounded-xl border-danger/30 bg-danger/5 px-4 py-3 text-sm text-danger">
              {files.error}
            </Panel>
          ) : null}

          {files.selectedFile ? (
            <FileDetailsPanel
              file={files.selectedFile}
              isDeleting={files.isDeleting}
              isUpdating={files.isUpdating}
              onClose={() => files.selectFile(null)}
              onDelete={files.deleteFile}
              onUpdate={files.updateFile}
            />
          ) : null}

          {files.isLoading ? (
            <Panel className="rounded-[var(--radius-panel)] border-border/80 bg-card/[0.78] p-14 text-center text-sm text-muted">
              {t("files:loading")}
            </Panel>
          ) : files.view === "grid" ? (
            <div className="files-page-grid grid gap-4">
              <button
                aria-label={t("files:upload")}
                className="rounded-[var(--radius-panel)] border border-dashed border-border/80 bg-card/60 p-4 text-left transition hover:border-brand/40 hover:bg-hover disabled:opacity-50"
                disabled={files.isUploading}
                onClick={() => fileInputRef.current?.click()}
                type="button"
              >
                <div className="flex h-full flex-col gap-4">
                  <div className="flex size-11 items-center justify-center rounded-2xl border border-dashed border-border bg-background/50 text-brand">
                    <Plus size={18} />
                  </div>
                  <div className="space-y-2">
                    <div className="text-base font-medium text-ink">
                      {files.isUploading ? t("files:uploading") : t("files:upload")}
                    </div>
                    <p className="text-sm leading-6 text-muted">{t("files:noFilesDetail")}</p>
                  </div>
                </div>
              </button>
              {files.filteredItems.map((file) => {
                const fileCategory = classifyFile(file);
                return (
                  <button
                    className={`rounded-[var(--radius-panel)] border p-4 text-left transition ${files.selectedFile?.fileId === file.fileId ? `${selectionClasses.selected} ${selectionClasses.border}` : "border-border/80 bg-card/[0.82] hover:border-selectedBorder"}`}
                    key={file.fileId}
                    onClick={() => files.selectFile(file.fileId)}
                    type="button"
                  >
                    <div className="flex items-start gap-3">
                      <div className="flex size-11 shrink-0 items-center justify-center rounded-2xl border border-border bg-background/50 text-brand">
                        {iconForFile(fileCategory)}
                      </div>
                      <div className="min-w-0">
                        <div className="truncate text-base font-medium text-ink">{file.originalFilename}</div>
                        <div className="mt-1 truncate font-mono text-xs text-muted">{file.mimeType}</div>
                      </div>
                    </div>
                    <div className="mt-4 flex items-center justify-between text-xs text-muted">
                      <span>{formatFileSize(file.sizeBytes)}</span>
                      <span>{formatFileMeta(file.updatedAt, locale)}</span>
                    </div>
                  </button>
                );
              })}
            </div>
          ) : (
            <Panel className="overflow-x-auto rounded-[var(--radius-panel)] border-border/80 bg-card/[0.82]">
              <div className="min-w-[640px]">
                <div className="grid grid-cols-[minmax(0,1.2fr)_minmax(0,1fr)_120px_110px] gap-4 border-b border-border/80 px-5 py-3 text-[11px] uppercase tracking-[0.18em] text-muted">
                  <div>{t("files:table.name")}</div>
                  <div>{t("files:table.type")}</div>
                  <div>{t("files:table.size")}</div>
                  <div>{t("files:table.updated")}</div>
                </div>
                <button
                  aria-label={t("files:upload")}
                  className="grid w-full grid-cols-[minmax(0,1.2fr)_minmax(0,1fr)_120px_110px] gap-4 border-b border-dashed border-border/80 bg-background/20 px-5 py-4 text-left transition hover:border-brand/40 hover:bg-hover disabled:opacity-50"
                  disabled={files.isUploading}
                  onClick={() => fileInputRef.current?.click()}
                  type="button"
                >
                  <div className="flex min-w-0 items-center gap-3">
                    <span className="flex size-8 shrink-0 items-center justify-center rounded-xl border border-dashed border-border bg-background/50 text-brand">
                      <Plus size={15} />
                    </span>
                    <span className="truncate text-sm font-medium text-ink">
                      {files.isUploading ? t("files:uploading") : t("files:upload")}
                    </span>
                  </div>
                  <div className="col-span-3 self-center truncate text-xs text-muted">
                    {t("files:noFilesDetail")}
                  </div>
                </button>
                {files.filteredItems.map((file) => (
                  <button
                    className={`grid w-full grid-cols-[minmax(0,1.2fr)_minmax(0,1fr)_120px_110px] gap-4 border-b px-5 py-4 text-left last:border-b-0 ${files.selectedFile?.fileId === file.fileId ? `${selectionClasses.selected} ${selectionClasses.border}` : "border-border/60 hover:bg-hover"}`}
                    key={file.fileId}
                    onClick={() => files.selectFile(file.fileId)}
                    type="button"
                  >
                    <div className="truncate text-sm font-medium text-ink">{file.originalFilename}</div>
                    <div className="truncate font-mono text-xs text-muted">{file.mimeType}</div>
                    <div className="text-xs text-muted">{formatFileSize(file.sizeBytes)}</div>
                    <div className="text-xs text-muted">{formatFileMeta(file.updatedAt, locale)}</div>
                  </button>
                ))}
              </div>
            </Panel>
          )}

          {files.hasMore ? (
            <button
              className="mx-auto rounded-xl border border-border bg-card/80 px-4 py-2 text-sm text-muted transition hover:text-ink disabled:opacity-50"
              disabled={files.isLoadingMore}
              onClick={() => void files.loadMore()}
              type="button"
            >
              {files.isLoadingMore ? t("files:loadingMore") : t("files:loadMore")}
            </button>
          ) : null}
        </div>
      </div>
    </div>
  );
}
