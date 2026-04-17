import { useMemo, useState } from "react";
import { Pencil, Plus, Search, Trash2 } from "lucide-react";

import { useAppStore } from "@/shared/store/app-store";
import type { MemoryRecord } from "@/shared/types/domain";
import { Button } from "@/shared/ui/Button";
import { Input, Textarea } from "@/shared/ui/Input";
import { Panel } from "@/shared/ui/Panel";

type MemorySection = "About you" | "Work preferences" | "Project context" | "Other";

function classifyMemory(item: MemoryRecord): MemorySection {
  const normalized = `${item.title} ${item.detail} ${item.tags.join(" ")}`.toLowerCase();
  if (normalized.includes("you") || normalized.includes("profile") || normalized.includes("engineer")) {
    return "About you";
  }
  if (normalized.includes("project") || normalized.includes("workspace") || normalized.includes("talos")) {
    return "Project context";
  }
  if (normalized.includes("prefer") || normalized.includes("review") || normalized.includes("style")) {
    return "Work preferences";
  }
  return "Other";
}

function formatMemoryDate(updatedAt: number) {
  return new Intl.DateTimeFormat("en", {
    month: "short",
    day: "2-digit",
    year: "numeric",
  }).format(updatedAt);
}

export function MemoryPage() {
  const items = useAppStore((state) => state.memoryItems);
  const upsertMemory = useAppStore((state) => state.upsertMemory);
  const removeMemory = useAppStore((state) => state.removeMemory);
  const [query, setQuery] = useState("");
  const [editingId, setEditingId] = useState<string | null>(null);
  const [draftTitle, setDraftTitle] = useState("");
  const [draftDetail, setDraftDetail] = useState("");

  const filteredItems = useMemo(() => {
    const normalized = query.trim().toLowerCase();
    if (!normalized) {
      return items;
    }
    return items.filter((item) =>
      [item.title, item.detail, item.tags.join(" ")].some((value) =>
        value.toLowerCase().includes(normalized),
      ),
    );
  }, [items, query]);

  const sections = useMemo(() => {
    const grouped = new Map<MemorySection, MemoryRecord[]>();
    for (const key of ["About you", "Work preferences", "Project context", "Other"] as MemorySection[]) {
      grouped.set(key, []);
    }
    filteredItems.forEach((item) => {
      grouped.get(classifyMemory(item))?.push(item);
    });
    return grouped;
  }, [filteredItems]);

  const hasQuery = query.trim().length > 0;
  const visibleSections = useMemo(
    () =>
      Array.from(sections.entries()).filter(([, records]) => hasQuery || records.length > 0),
    [hasQuery, sections],
  );

  function beginEdit(item: MemoryRecord) {
    setEditingId(item.id);
    setDraftTitle(item.title);
    setDraftDetail(item.detail);
  }

  function commitEdit(item: MemoryRecord) {
    upsertMemory({
      ...item,
      title: draftTitle.trim() || item.title,
      detail: draftDetail.trim() || item.detail,
      updatedAt: Date.now(),
    });
    setEditingId(null);
  }

  function addMemory() {
    const id = `memory-${Date.now()}`;
    upsertMemory({
      id,
      title: "New memory",
      detail: "Add a durable fact that Talos should remember across sessions.",
      tags: ["other"],
      updatedAt: Date.now(),
    });
    setEditingId(id);
    setDraftTitle("New memory");
    setDraftDetail("Add a durable fact that Talos should remember across sessions.");
  }

  const totalCount = items.length;

  return (
    <div className="mx-auto flex h-full w-full max-w-[1080px] flex-col gap-5 overflow-y-auto px-8 py-7">
      <Panel className="rounded-[1.45rem] border-border/80 bg-card/88 p-5 backdrop-blur">
        <div className="flex flex-wrap items-start justify-between gap-4">
          <div>
            <div className="text-xs uppercase tracking-[0.22em] text-muted">Memory</div>
            <h1 className="mt-2 font-display text-[28px] tracking-[-0.05em] text-ink">
              Durable notes the assistant keeps about you
            </h1>
            <p className="mt-3 max-w-2xl text-sm leading-6 text-muted">
              Review the facts Talos should carry across conversations. Search, edit, or remove them before
              they influence future replies.
            </p>
          </div>
          <Button onClick={addMemory} variant="secondary">
            <Plus size={16} />
            Add memory
          </Button>
        </div>
      </Panel>

      <div className="grid gap-5 xl:grid-cols-[250px_minmax(0,1fr)]">
        <div className="space-y-5">
          <Panel className="rounded-[1.25rem] border-border/65 bg-card/70 p-4 shadow-[0_10px_30px_-24px_rgba(15,23,42,0.34)]">
            <div className="text-[11px] uppercase tracking-[0.22em] text-muted/80">Overview</div>
            <div className="mt-3 flex items-end gap-2.5">
              <div className="text-[2.1rem] font-display leading-none tracking-[-0.06em] text-ink">{totalCount}</div>
              <div className="pb-0.5 text-[11px] uppercase tracking-[0.14em] text-muted/80">Active notes</div>
            </div>
            <div className="mt-4 flex flex-wrap gap-1.5">
              {Array.from(sections.entries()).map(([section, records]) => (
                <div
                  className="rounded-full border border-border/60 bg-background/78 px-2.5 py-1 text-[11px] leading-5"
                  key={section}
                >
                  <span className="text-muted/90">{section}</span>
                  <span className="ml-1.5 font-medium text-ink">{records.length}</span>
                </div>
              ))}
            </div>
          </Panel>

          <Panel className="rounded-[1.25rem] border-border/65 bg-card/70 p-4 shadow-[0_10px_30px_-24px_rgba(15,23,42,0.34)]">
            <label className="mb-3 block text-[11px] uppercase tracking-[0.22em] text-muted/80">Search</label>
            <div className="relative">
              <Search className="pointer-events-none absolute left-3.5 top-1/2 -translate-y-1/2 text-muted/80" size={14} />
              <Input
                className="h-11 rounded-[1rem] border-border/60 bg-background/82 pl-10"
                onChange={(event) => setQuery(event.target.value)}
                placeholder="Search memory..."
                value={query}
              />
            </div>
            <button
              className="mt-3 inline-flex w-full items-center justify-center rounded-[1rem] border border-transparent px-3 py-2 text-sm text-muted transition hover:border-danger/12 hover:bg-danger/6 hover:text-danger"
              onClick={() => {
                items.forEach((item) => removeMemory(item.id));
                setEditingId(null);
              }}
              type="button"
            >
              Clear all memories
            </button>
          </Panel>
        </div>

        <div className="space-y-5">
          {visibleSections.map(([section, records]) => (
            <section key={section}>
              <div className="mb-3 flex items-center justify-between">
                <h2 className="text-sm font-medium uppercase tracking-[0.18em] text-muted">{section}</h2>
                <span className="text-xs text-muted">{records.length}</span>
              </div>
              {records.length === 0 ? (
                <Panel className="rounded-[1.4rem] border-border/80 bg-card/70 p-8 text-center">
                  <div className="text-sm text-muted">Nothing matched this section.</div>
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
                                    Save
                                  </Button>
                                  <Button onClick={() => setEditingId(null)} type="button" variant="ghost">
                                    Cancel
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
                              <span className="ml-auto">Recorded {formatMemoryDate(item.updatedAt)}</span>
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
              <h2 className="mt-4 font-display text-2xl tracking-[-0.04em] text-ink">No memory yet</h2>
              <p className="mx-auto mt-3 max-w-sm text-sm leading-6 text-muted">
                Start a few conversations and Talos will begin capturing facts worth carrying forward.
              </p>
            </Panel>
          ) : null}
        </div>
      </div>
    </div>
  );
}
