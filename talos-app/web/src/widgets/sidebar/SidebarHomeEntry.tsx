import { Home } from "lucide-react";
import { Link } from "react-router-dom";
import { useTranslation } from "react-i18next";

import { cn } from "@/shared/lib/cn";
import { sidebarInteractiveRowClass } from "./sidebarRowStyles";

export function SidebarHomeEntry({ computer }: { computer?: import("@talos/sdk-core").AppProtocol.NodeInfo }) {
  const { t } = useTranslation("computers");

  return (
    <Link
      className={cn(
        sidebarInteractiveRowClass,
        "mb-6 grid grid-cols-[2.25rem_minmax(0,1fr)] items-center gap-3 px-1.5 py-1.5 text-ink",
      )}
      to="/"
    >
      <span className="flex size-9 items-center justify-center rounded-[0.7rem] border border-border/75 bg-surface text-ink">
        <Home aria-hidden="true" size={17} />
      </span>
      <span className="min-w-0">
        <span className="block truncate text-sm font-semibold leading-5">Talos</span>
        <span className="mt-0.5 flex items-center gap-2 text-xs leading-4 text-muted">
          <span className="truncate" title={computer?.displayName}>{computer ? t(computer.available ? "connectedTo" : "offlineComputer", { name: computer.displayName }) : t("choose")}</span>
          <span className={`size-1.5 shrink-0 rounded-full ${computer?.available ? "bg-success" : "bg-muted/50"}`} />
        </span>
      </span>
    </Link>
  );
}
