import { Home } from "lucide-react";
import { Link } from "react-router-dom";
import { useTranslation } from "react-i18next";

export function SidebarHomeEntry() {
  const { t } = useTranslation("common");

  return (
    <Link
      className="mb-6 grid grid-cols-[2.25rem_minmax(0,1fr)] items-center gap-3 rounded-[var(--radius-control)] px-1.5 py-1.5 text-ink transition hover:bg-card/70 focus-visible:ring-2 focus-visible:ring-brand/30"
      to="/"
    >
      <span className="flex size-9 items-center justify-center rounded-[0.7rem] border border-border/75 bg-background/70 text-ink">
        <Home aria-hidden="true" size={17} />
      </span>
      <span className="min-w-0">
        <span className="block truncate text-sm font-semibold leading-5">Talos</span>
        <span className="mt-0.5 flex items-center gap-2 text-xs leading-4 text-muted">
          {t("connected")}
          <span className="size-2 rounded-full bg-success" />
        </span>
      </span>
    </Link>
  );
}
