import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { useAppStore } from "@/shared/store/app-store";
import type { MemoryRecord } from "@/shared/types/ui";

export type MemorySectionId = "aboutYou" | "workPreferences" | "projectContext" | "other";

export function classifyMemory(item: MemoryRecord): MemorySectionId {
  const normalized = `${item.title} ${item.detail} ${item.tags.join(" ")}`.toLowerCase();
  if (normalized.includes("you") || normalized.includes("profile") || normalized.includes("engineer")) {
    return "aboutYou";
  }
  if (normalized.includes("project") || normalized.includes("workspace") || normalized.includes("talos")) {
    return "projectContext";
  }
  if (normalized.includes("prefer") || normalized.includes("review") || normalized.includes("style")) {
    return "workPreferences";
  }
  return "other";
}

export function formatMemoryDate(updatedAt: number) {
  const locale = typeof navigator !== "undefined" ? navigator.language : "en";
  return new Intl.DateTimeFormat(locale, {
    month: "short",
    day: "2-digit",
    year: "numeric",
  }).format(updatedAt);
}

export function useMemoryPage() {
  const { t } = useTranslation("memory");
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
    const grouped = new Map<MemorySectionId, MemoryRecord[]>();
    for (const key of ["aboutYou", "workPreferences", "projectContext", "other"] as MemorySectionId[]) {
      grouped.set(key, []);
    }
    filteredItems.forEach((item) => {
      grouped.get(classifyMemory(item))?.push(item);
    });
    return grouped;
  }, [filteredItems]);

  const hasQuery = query.trim().length > 0;
  const visibleSections = useMemo(
    () => Array.from(sections.entries()).filter(([, records]) => hasQuery || records.length > 0),
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

  function cancelEdit() {
    setEditingId(null);
  }

  function addMemory() {
    const id = `memory-${Date.now()}`;
    upsertMemory({
      id,
      title: t("newMemoryTitle"),
      detail: t("newMemoryDetail"),
      tags: ["other"],
      updatedAt: Date.now(),
    });
    setEditingId(id);
    setDraftTitle(t("newMemoryTitle"));
    setDraftDetail(t("newMemoryDetail"));
  }

  function clearMemories() {
    items.forEach((item) => removeMemory(item.id));
    setEditingId(null);
  }

  return {
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
  };
}
