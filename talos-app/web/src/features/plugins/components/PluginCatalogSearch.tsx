import { Filter, Search } from "lucide-react";
import { useTranslation } from "react-i18next";

interface PluginCatalogSearchProps {
  query: string;
  onQueryChange: (query: string) => void;
}

export function PluginCatalogSearch({ query, onQueryChange }: PluginCatalogSearchProps) {
  const { t } = useTranslation("plugins");

  return (
    <div className="mt-5 flex items-center gap-2">
      <div className="relative flex-1">
        <Search
          className="pointer-events-none absolute left-3 top-1/2 -translate-y-1/2 text-muted"
          size={16}
        />
        <input
          className="h-11 w-full rounded-full border border-border/80 bg-card pl-10 pr-4 text-sm outline-none transition placeholder:text-muted focus:border-brand/45"
          onChange={(event) => onQueryChange(event.target.value)}
          placeholder={t("searchPlaceholder")}
          value={query}
        />
      </div>
      <button
        className="inline-flex h-11 w-11 items-center justify-center rounded-full border border-border/80 bg-card text-muted transition hover:border-brand/35 hover:text-ink"
        type="button"
      >
        <Filter size={17} />
      </button>
    </div>
  );
}
