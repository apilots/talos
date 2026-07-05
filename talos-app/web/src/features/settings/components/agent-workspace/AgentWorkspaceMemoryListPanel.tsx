import { Search } from "lucide-react";
import { useTranslation } from "react-i18next";

import { useMemoryPage } from "@/features/memory";
import { Input } from "@/shared/ui/Input";
import { Panel } from "@/shared/ui/Panel";

type MemoryPageState = ReturnType<typeof useMemoryPage>;

interface AgentWorkspaceMemoryListPanelProps {
  dates: string[];
  memory: MemoryPageState;
  showingSearch: boolean;
}

export function AgentWorkspaceMemoryListPanel({
  dates,
  memory,
  showingSearch,
}: AgentWorkspaceMemoryListPanelProps) {
  const { t } = useTranslation(["common", "memory"]);
  const busy = memory.isSearchPending || memory.isSearching;

  return (
    <Panel className="rounded-[var(--radius-panel)] border-border/70 bg-card/78 p-2.5">
      <div className="px-2 pt-2">
        <div className="text-[11px] uppercase tracking-[0.22em] text-muted/80">{t("common:search")}</div>
        <div className="relative mt-3">
          <Search className="pointer-events-none absolute left-3.5 top-1/2 -translate-y-1/2 text-muted/80" size={14} />
          <Input
            className="h-11 rounded-[var(--radius-control)] border-border/65 bg-background/82 pl-10"
            onChange={(event) => memory.setQuery(event.target.value)}
            placeholder={t("memory:searchPlaceholder")}
            value={memory.query}
          />
        </div>
        <div className="mt-3 text-xs text-muted">
          {showingSearch
            ? busy
              ? t("memory:searching")
              : t("memory:searchResultsCount", { count: memory.searchResults.length })
            : t("memory:fileCount", { count: memory.files.length })}
        </div>
      </div>
      <div className="mt-4 space-y-1">
        {memory.isLoadingList ? (
          <div className="px-3 py-4 text-sm text-muted">{t("common:loading")}</div>
        ) : dates.length === 0 ? (
          <div className="px-3 py-4 text-sm text-muted">{t("memory:noMemoryDetail")}</div>
        ) : (
          dates.map((date) => (
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
              <div className="mt-1 text-xs text-muted">{t("memory:dailyMemory")}</div>
            </button>
          ))
        )}
      </div>
    </Panel>
  );
}
