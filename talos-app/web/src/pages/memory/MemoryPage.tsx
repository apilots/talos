import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { Pencil, Plus, Search, Trash2 } from "lucide-react";

import { formatMemoryDate, useMemoryPage, type MemorySectionId } from "@/features/memory";
import { Button } from "@/shared/ui/Button";
import { Input, Textarea } from "@/shared/ui/Input";
import { Panel } from "@/shared/ui/Panel";

export function MemoryPage() {
  const { t } = useTranslation(["memory", "common"]);
  const {
    items,
    query,
    setQuery,
    filteredItems,
    visibleSections,
    editingId,
    draftTitle,
    draftDetail,
    setDraftTitle,
    setDraftDetail,
    beginEdit,
    commitEdit,
    cancelEdit,
    addMemory,
    clearMemories,
    removeMemory,
  } = useMemoryPage();

  const totalCount = items.length;
  const sectionLabels = useMemo<Record<MemorySectionId, string>>(
    () => ({
      aboutYou: t("memory:sections.aboutYou"),
      workPreferences: t("memory:sections.workPreferences"),
      projectContext: t("memory:sections.projectContext"),
      other: t("memory:sections.other"),
    }),
    [t],
  );

  return (
    <div className="mx-auto flex h-full w-full max-w-[1080px] flex-col gap-5 overflow-y-auto px-8 py-7">
      <Panel className="rounded-[1.45rem] border-border/80 bg-card/88 p-5 backdrop-blur">
        <div className="flex flex-wrap items-start justify-between gap-4">
          <div>
            <div className="text-xs uppercase tracking-[0.22em] text-muted">{t("memory:pageLabel")}</div>
            <h1 className="mt-2 font-display text-[28px] tracking-[-0.05em] text-ink">
              {t("memory:title")}
            </h1>
            <p className="mt-3 max-w-2xl text-sm leading-6 text-muted">
              {t("memory:detail")}
            </p>
          </div>
          <Button onClick={addMemory} variant="secondary">
            <Plus size={16} />
            {t("memory:addMemory")}
          </Button>
        </div>
      </Panel>

      <div className="grid gap-5 xl:grid-cols-[250px_minmax(0,1fr)]">
        <div className="space-y-5">
          <Panel className="rounded-[1.25rem] border-border/65 bg-card/70 p-4 shadow-[0_10px_30px_-24px_rgba(15,23,42,0.34)]">
            <div className="text-[11px] uppercase tracking-[0.22em] text-muted/80">{t("memory:overview")}</div>
            <div className="mt-3 flex items-end gap-2.5">
              <div className="text-[2.1rem] font-display leading-none tracking-[-0.06em] text-ink">{totalCount}</div>
              <div className="pb-0.5 text-[11px] uppercase tracking-[0.14em] text-muted/80">{t("memory:activeNotes")}</div>
            </div>
            <div className="mt-4 flex flex-wrap gap-1.5">
              {visibleSections.map(([section, records]) => (
                <div
                  className="rounded-full border border-border/60 bg-background/78 px-2.5 py-1 text-[11px] leading-5"
                  key={section}
                >
                  <span className="text-muted/90">{sectionLabels[section]}</span>
                  <span className="ml-1.5 font-medium text-ink">{records.length}</span>
                </div>
              ))}
            </div>
          </Panel>

          <Panel className="rounded-[1.25rem] border-border/65 bg-card/70 p-4 shadow-[0_10px_30px_-24px_rgba(15,23,42,0.34)]">
            <label className="mb-3 block text-[11px] uppercase tracking-[0.22em] text-muted/80">{t("common:search")}</label>
            <div className="relative">
              <Search className="pointer-events-none absolute left-3.5 top-1/2 -translate-y-1/2 text-muted/80" size={14} />
              <Input
                className="h-11 rounded-[1rem] border-border/60 bg-background/82 pl-10"
                onChange={(event) => setQuery(event.target.value)}
                placeholder={t("memory:searchPlaceholder")}
                value={query}
              />
            </div>
            <button
              className="mt-3 inline-flex w-full items-center justify-center rounded-[1rem] border border-transparent px-3 py-2 text-sm text-muted transition hover:border-danger/12 hover:bg-danger/6 hover:text-danger"
              onClick={clearMemories}
              type="button"
            >
              {t("memory:clearAll")}
            </button>
          </Panel>
        </div>

        <div className="space-y-5">
          {visibleSections.map(([section, records]) => (
            <section key={section}>
              <div className="mb-3 flex items-center justify-between">
                <h2 className="text-sm font-medium uppercase tracking-[0.18em] text-muted">{sectionLabels[section]}</h2>
                <span className="text-xs text-muted">{records.length}</span>
              </div>
              {records.length === 0 ? (
                <Panel className="rounded-[1.4rem] border-border/80 bg-card/70 p-8 text-center">
                  <div className="text-sm text-muted">{t("memory:nothingMatched")}</div>
                </Panel>
              ) : (
                <div className="space-y-3">
                  {records.map((item) => {
                    const isEditing = editingId === item.id;
                    return (
                      <Panel className="rounded-[1.3rem] border-border/80 bg-card/82 p-4" key={item.id}>
                        <div className="flex items-start justify-between gap-4">
                          <div className="min-w-0 flex-1">
                            {isEditing ? (
                              <div className="space-y-3">
                                <Input
                                  onChange={(event) => setDraftTitle(event.target.value)}
                                  value={draftTitle}
                                />
                                <Textarea
                                  className="min-h-24"
                                  onChange={(event) => setDraftDetail(event.target.value)}
                                  value={draftDetail}
                                />
                                <div className="flex items-center gap-2">
                                  <Button onClick={() => commitEdit(item)} type="button">
                                    {t("common:save")}
                                  </Button>
                                  <Button onClick={cancelEdit} type="button" variant="ghost">
                                    {t("common:cancel")}
                                  </Button>
                                </div>
                              </div>
                            ) : (
                              <>
                                <div className="text-base font-medium text-ink">{item.title}</div>
                                <p className="mt-2 text-sm leading-6 text-muted">{item.detail}</p>
                              </>
                            )}
                            <div className="mt-4 flex flex-wrap items-center gap-2 text-xs text-muted">
                              {item.tags.map((tag) => (
                                <span
                                  className="rounded-full border border-border/70 bg-background/55 px-2.5 py-1"
                                  key={`${item.id}-${tag}`}
                                >
                                  {tag}
                                </span>
                              ))}
                              <span className="ml-auto">{t("memory:recordedOn", { date: formatMemoryDate(item.updatedAt) })}</span>
                            </div>
                          </div>
                          <div className="flex items-center gap-1">
                            <button
                              className="rounded-xl p-2 text-muted transition hover:bg-surface hover:text-ink"
                              onClick={() => beginEdit(item)}
                              type="button"
                            >
                              <Pencil size={15} />
                            </button>
                            <button
                              className="rounded-xl p-2 text-muted transition hover:bg-danger/10 hover:text-danger"
                              onClick={() => removeMemory(item.id)}
                              type="button"
                            >
                              <Trash2 size={15} />
                            </button>
                          </div>
                        </div>
                      </Panel>
                    );
                  })}
                </div>
              )}
            </section>
          ))}

          {filteredItems.length === 0 ? (
            <Panel className="rounded-[1.4rem] border-border/80 bg-card/80 p-12 text-center">
              <div className="text-4xl">🗂</div>
              <h2 className="mt-4 font-display text-2xl tracking-[-0.04em] text-ink">{t("memory:noMemoryTitle")}</h2>
              <p className="mx-auto mt-3 max-w-sm text-sm leading-6 text-muted">
                {t("memory:noMemoryDetail")}
              </p>
            </Panel>
          ) : null}
        </div>
      </div>
    </div>
  );
}
