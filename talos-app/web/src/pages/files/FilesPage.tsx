import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { FileCode2, FileImage, FileSpreadsheet, FileText, Grid2x2, List, Plus, Trash2, Upload } from "lucide-react";

import { classifyFile, formatFileMeta, useFilesPage, type FileCategory } from "@/features/files";
import { Input } from "@/shared/ui/Input";
import { Panel } from "@/shared/ui/Panel";

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
  const { t } = useTranslation(["files", "common"]);
  const { query, setQuery, view, setView, category, setCategory, categorized, filteredItems, addFile, removeFile } =
    useFilesPage();
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

  return (
    <div className="flex h-full w-full flex-col overflow-hidden">
      <div className="min-h-0 flex-1 overflow-y-auto">
        <div className="mx-auto flex w-full max-w-[1120px] flex-col gap-5 px-8 py-7">
      <Panel className="rounded-[1.35rem] border-border/80 bg-card/82 px-5 py-4">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <Input
            className="min-w-0 flex-1"
            onChange={(event) => setQuery(event.target.value)}
            placeholder={t("files:searchPlaceholder")}
            value={query}
          />
          <div className="flex flex-wrap items-center gap-2">
            <div className="flex items-center gap-1.5">
              <button
                className={[
                  "rounded-xl border p-2 transition",
                  view === "list"
                    ? "border-brand/40 bg-brand/10 text-brand"
                    : "border-border bg-card/80 text-muted hover:border-brand/30 hover:text-ink",
                ].join(" ")}
                onClick={() => setView("list")}
                type="button"
              >
                <List size={15} />
              </button>
              <button
                className={[
                  "rounded-xl border p-2 transition",
                  view === "grid"
                    ? "border-brand/40 bg-brand/10 text-brand"
                    : "border-border bg-card/80 text-muted hover:border-brand/30 hover:text-ink",
                ].join(" ")}
                onClick={() => setView("grid")}
                type="button"
              >
                <Grid2x2 size={15} />
              </button>
            </div>
            <div className="h-5 w-px bg-border/70" />
            <div className="flex flex-wrap items-center gap-1.5">
              {(Object.keys(categoryLabels) as FileCategory[]).map((itemCategory) => (
                <button
                  className={[
                    "rounded-full border px-3 py-1.5 text-xs transition",
                    category === itemCategory
                      ? "border-brand/40 bg-brand/10 text-brand"
                      : "border-border bg-background/55 text-muted hover:text-ink",
                  ].join(" ")}
                  key={itemCategory}
                  onClick={() => setCategory(itemCategory)}
                  type="button"
                >
                  {categoryLabels[itemCategory]} ({categorized[itemCategory]})
                </button>
              ))}
            </div>
          </div>
        </div>
      </Panel>

      {filteredItems.length === 0 ? (
        <Panel className="rounded-[1.45rem] border-border/80 bg-card/78 p-14 text-center">
          <div className="mx-auto flex size-14 items-center justify-center rounded-3xl border border-border bg-background/55 text-brand">
            <Upload size={22} />
          </div>
          <h2 className="mt-5 font-display text-2xl tracking-[-0.04em] text-ink">{t("files:noFilesTitle")}</h2>
          <p className="mx-auto mt-3 max-w-sm text-sm leading-6 text-muted">{t("files:noFilesDetail")}</p>
        </Panel>
      ) : view === "grid" ? (
        <div className="grid gap-4 md:grid-cols-2 xl:grid-cols-3">
          <button
            className="rounded-[1.3rem] border border-dashed border-border/80 bg-card/60 p-4 text-left transition hover:border-brand/40 hover:bg-card/90"
            onClick={addFile}
            type="button"
          >
            <div className="flex h-full flex-col gap-4">
              <div className="flex size-11 items-center justify-center rounded-2xl border border-dashed border-border bg-background/50 text-brand">
                <Plus size={18} />
              </div>
              <div className="space-y-2">
                <div className="text-base font-medium text-ink">{t("files:upload")}</div>
                <p className="text-sm leading-6 text-muted">{t("files:dragHint")}</p>
              </div>
            </div>
          </button>
          {filteredItems.map((item) => {
            const fileCategory = classifyFile(item);
            return (
              <Panel className="rounded-[1.3rem] border-border/80 bg-card/82 p-4" key={item.id}>
                <div className="flex h-full flex-col gap-4">
                  <div className="flex items-start justify-between gap-3">
                    <div className="flex size-11 items-center justify-center rounded-2xl border border-border bg-background/50 text-brand">
                      {iconForFile(fileCategory)}
                    </div>
                    <button
                      className="rounded-xl p-2 text-muted transition hover:bg-danger/10 hover:text-danger"
                      onClick={() => removeFile(item.id)}
                      type="button"
                    >
                      <Trash2 size={15} />
                    </button>
                  </div>
                  <div className="space-y-2">
                    <div className="text-base font-medium text-ink">{item.label}</div>
                    <div className="font-mono text-xs text-muted">{item.path}</div>
                    <p className="line-clamp-3 text-sm leading-6 text-muted">{item.summary}</p>
                  </div>
                  <div className="mt-auto flex items-center justify-between text-xs text-muted">
                    <span>{categoryLabels[fileCategory]}</span>
                    <span>{formatFileMeta(item.updatedAt)}</span>
                  </div>
                </div>
              </Panel>
            );
          })}
        </div>
      ) : (
        <Panel className="overflow-hidden rounded-[1.4rem] border-border/80 bg-card/82">
          <div className="grid grid-cols-[minmax(0,1.2fr)_minmax(0,1.4fr)_140px_120px] gap-4 border-b border-border/80 px-5 py-3 text-[11px] uppercase tracking-[0.18em] text-muted">
            <div>{t("files:table.name")}</div>
            <div>{t("files:table.path")}</div>
            <div>{t("files:table.type")}</div>
            <div className="text-right">{t("files:table.actions")}</div>
          </div>
          {filteredItems.map((item) => {
            const fileCategory = classifyFile(item);
            return (
              <div
                className="grid grid-cols-[minmax(0,1.2fr)_minmax(0,1.4fr)_140px_120px] gap-4 border-b border-border/60 px-5 py-4 last:border-b-0"
                key={item.id}
              >
                <div className="min-w-0">
                  <div className="truncate text-sm font-medium text-ink">{item.label}</div>
                  <div className="mt-1 truncate text-xs text-muted">{item.summary}</div>
                </div>
                <div className="truncate font-mono text-xs text-muted">{item.path}</div>
                <div className="text-sm text-muted">{categoryLabels[fileCategory]}</div>
                <div className="flex items-center justify-end gap-2">
                  <span className="text-xs text-muted">{t("files:ready")}</span>
                  <button
                    className="rounded-xl p-2 text-muted transition hover:bg-danger/10 hover:text-danger"
                    onClick={() => removeFile(item.id)}
                    type="button"
                  >
                    <Trash2 size={15} />
                  </button>
                </div>
              </div>
            );
          })}
        </Panel>
      )}

        </div>
      </div>
    </div>
  );
}
