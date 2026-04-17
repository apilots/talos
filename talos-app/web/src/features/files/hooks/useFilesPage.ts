import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { useAppStore } from "@/shared/store/app-store";
import type { FileRecord } from "@/shared/types/ui";

export type FileView = "grid" | "list";
export type FileCategory = "all" | "document" | "code" | "image" | "data";

export function getFileExtension(path: string) {
  return path.split(".").at(-1)?.toLowerCase() ?? "";
}

export function classifyFile(item: FileRecord): FileCategory {
  const ext = getFileExtension(item.path);
  if (["rs", "ts", "tsx", "py", "js", "jsx", "yaml", "yml"].includes(ext)) {
    return "code";
  }
  if (["png", "jpg", "jpeg", "gif", "webp", "svg"].includes(ext)) {
    return "image";
  }
  if (["csv", "json"].includes(ext)) {
    return "data";
  }
  return "document";
}

export function formatFileMeta(updatedAt: number) {
  const locale = typeof navigator !== "undefined" ? navigator.language : "en";
  return new Intl.DateTimeFormat(locale, {
    month: "short",
    day: "2-digit",
  }).format(updatedAt);
}

export function useFilesPage() {
  const { t } = useTranslation("files");
  const items = useAppStore((state) => state.fileItems);
  const upsertFile = useAppStore((state) => state.upsertFile);
  const removeFile = useAppStore((state) => state.removeFile);
  const [query, setQuery] = useState("");
  const [view, setView] = useState<FileView>("grid");
  const [category, setCategory] = useState<FileCategory>("all");

  const categorized = useMemo(() => {
    const counts: Record<FileCategory, number> = {
      all: items.length,
      document: 0,
      code: 0,
      image: 0,
      data: 0,
    };
    items.forEach((item) => {
      counts[classifyFile(item)] += 1;
    });
    return counts;
  }, [items]);

  const filteredItems = useMemo(() => {
    const normalized = query.trim().toLowerCase();
    return items.filter((item) => {
      const matchQuery =
        !normalized ||
        [item.label, item.path, item.summary].some((value) => value.toLowerCase().includes(normalized));
      const matchCategory = category === "all" || classifyFile(item) === category;
      return matchQuery && matchCategory;
    });
  }, [category, items, query]);

  function addFile() {
    const id = `file-${Date.now()}`;
    const index = items.length + 1;
    upsertFile({
      id,
      label: t("placeholders.newFileLabel", { index }),
      path: t("placeholders.newFilePath", { index }),
      summary: t("placeholders.newFileSummary"),
      updatedAt: Date.now(),
    });
  }

  return {
    query,
    setQuery,
    view,
    setView,
    category,
    setCategory,
    categorized,
    filteredItems,
    addFile,
    removeFile,
  };
}
