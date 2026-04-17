import type { ReactNode } from "react";
import { HardDriveUpload, MemoryStick, Plus, Settings2, Shield, SlidersHorizontal, Trash2 } from "lucide-react";
import { Link, NavLink } from "react-router-dom";
import { useTranslation } from "react-i18next";

import { formatRelativeTime } from "@/shared/lib/time";
import { cn } from "@/shared/lib/cn";
import { Badge } from "@/shared/ui/Badge";
import type { SessionUiStatus, ThreadFilter } from "@/shared/types/ui";

function statusTone(status: string) {
  if (status === "running") {
    return "success";
  }
  if (status === "waiting_approval") {
    return "warning";
  }
  if (status === "error") {
    return "danger";
  }
  return "default";
}

export interface SidebarThreadItem {
  id: string;
  label: string;
  status: SessionUiStatus;
  model: string;
  lastActivityAt: number;
  createdAt: number;
}

interface SidebarProps {
  currentPath: string;
  isHomePage: boolean;
  threadCount: number;
  threadFilter: ThreadFilter;
  threads: SidebarThreadItem[];
  onCycleThreadFilter: () => void;
  onCreateThread: () => Promise<void> | void;
  onSelectThread: (sessionId: string) => void;
  onDeleteThread: (sessionId: string) => Promise<void> | void;
}

export function Sidebar({
  currentPath,
  isHomePage,
  threadCount,
  threadFilter,
  threads,
  onCycleThreadFilter,
  onCreateThread,
  onSelectThread,
  onDeleteThread,
}: SidebarProps) {
  const { t } = useTranslation(["sidebar", "common"]);
  const filterLabel =
    threadFilter === "all"
      ? t("sidebar:allThreads")
      : threadFilter === "waiting_approval"
        ? t("sidebar:awaitingApproval")
        : threadFilter.replaceAll("_", " ");

  return (
    <aside className="sticky top-0 flex h-screen w-full shrink-0 flex-col border-r border-border/80 bg-sidebar/95 px-4 py-4 backdrop-blur-[18px]">
      <Link
        className="mb-5 block rounded-[var(--radius-panel)] border border-border/75 bg-card/86 px-4 py-4 shadow-[0_12px_28px_rgba(95,84,64,0.04)]"
        to="/"
      >
        <div className="flex items-center justify-between gap-3">
          <div>
            <div className="font-display text-[21px] font-semibold tracking-[-0.04em] text-ink">Talos</div>
            <div className="mt-1 text-[10px] uppercase tracking-[0.22em] text-muted">{t("sidebar:webConsole")}</div>
          </div>
          <div className="flex size-10 items-center justify-center rounded-[var(--radius-control)] border border-brand/25 bg-brand/10 text-brand">
            ⬡
          </div>
        </div>
        <div className="mt-3 flex items-center gap-2 text-[11px] text-muted">
          <span className="size-2 rounded-full bg-success shadow-[0_0_14px_rgba(94,234,212,0.55)]" />
          <span>{t("common:connected")}</span>
        </div>
      </Link>

      <div className="mb-3 flex items-end justify-between gap-3">
        <div>
          <div className="text-xs uppercase tracking-[0.18em] text-muted">{t("sidebar:threads")}</div>
          <div className="mt-1 text-[22px] font-display font-semibold tracking-[-0.04em] text-ink">
            {threadCount}
          </div>
          <div className="mt-1 text-[11px] text-muted">{filterLabel}</div>
        </div>
        <div className="flex items-center gap-2">
          <button
            className="rounded-[var(--radius-control)] border border-border/80 bg-card/88 p-2.5 text-muted transition hover:border-brand/40 hover:text-ink"
            onClick={onCycleThreadFilter}
            type="button"
          >
            <SlidersHorizontal size={16} />
          </button>
          {!isHomePage ? (
            <button
              className="rounded-[var(--radius-control)] border border-border/80 bg-card/88 p-2.5 text-muted transition hover:border-brand/40 hover:text-ink"
              onClick={() => void onCreateThread()}
              type="button"
            >
              <Plus size={16} />
            </button>
          ) : null}
        </div>
      </div>

      <div className="min-h-0 flex-1 space-y-2 overflow-y-auto pr-1.5">
        {threads.map((thread) => {
          return (
            <div
              className={cn(
                "group cursor-pointer rounded-[var(--radius-control)] border border-transparent bg-transparent px-3.5 py-3 transition hover:border-border/80 hover:bg-card/88",
                currentPath === `/sessions/${thread.id}` &&
                  "border-border/85 bg-card/94 shadow-[0_10px_24px_rgba(95,84,64,0.045)]",
              )}
              onClick={() => onSelectThread(thread.id)}
              key={`${thread.id}-${thread.createdAt}`}
              role="button"
              tabIndex={0}
              onKeyDown={(event) => {
                if (event.key === "Enter" || event.key === " ") {
                  event.preventDefault();
                  onSelectThread(thread.id);
                }
              }}
            >
              <div className="flex items-center justify-between gap-2">
                <span className="line-clamp-2 text-[13px] font-medium leading-5 text-ink">{thread.label}</span>
                <Badge tone={statusTone(thread.status)}>{thread.status.replaceAll("_", " ")}</Badge>
              </div>
              <div className="mt-1.5 flex items-center justify-between gap-3 text-[10px] text-muted">
                <span className="truncate">{thread.model}</span>
                <span>{formatRelativeTime(thread.lastActivityAt)}</span>
              </div>
              <div className="mt-1.5 flex items-center justify-end text-xs opacity-0 transition group-hover:opacity-100 group-focus-within:opacity-100">
                <button
                  className="inline-flex items-center gap-1 rounded-[var(--radius-control)] px-2 py-1 text-muted transition hover:bg-danger/10 hover:text-danger"
                  onClick={async (event) => {
                    event.stopPropagation();
                    await onDeleteThread(thread.id);
                  }}
                  type="button"
                >
                  <Trash2 size={14} />
                  {t("common:delete")}
                </button>
              </div>
            </div>
          );
        })}
      </div>

      <nav className="mt-5 space-y-1 border-t border-border/80 pt-5">
        <NavItem icon={<MemoryStick size={16} />} label={t("sidebar:memory")} to="/memory" />
        <NavItem icon={<HardDriveUpload size={16} />} label={t("sidebar:files")} to="/files" />
        <NavItem icon={<Settings2 size={16} />} label={t("sidebar:settings")} to="/settings" />
        <NavItem icon={<Shield size={16} />} label={t("sidebar:admin")} to="/admin" />
      </nav>
    </aside>
  );
}

function NavItem({
  icon,
  label,
  to,
}: {
  icon: ReactNode;
  label: string;
  to: string;
}) {
  return (
    <NavLink
      className={({ isActive }) =>
        cn(
          "flex items-center gap-3 rounded-[var(--radius-control)] px-3 py-2.5 text-sm text-muted transition hover:bg-card/88 hover:text-ink",
          isActive && "bg-card/92 text-ink shadow-[inset_0_0_0_1px_rgba(224,218,209,0.5)]",
        )
      }
      to={to}
    >
      {icon}
      <span>{label}</span>
    </NavLink>
  );
}
