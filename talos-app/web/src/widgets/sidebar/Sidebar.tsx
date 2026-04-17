import type { ReactNode } from "react";
import { HardDriveUpload, MemoryStick, Plus, Settings2, Shield, SlidersHorizontal, Trash2 } from "lucide-react";
import { Link, NavLink, useLocation, useNavigate } from "react-router-dom";

import { useCreateSessionMutation, useModelsQuery, useSessionsQuery, useCloseSessionMutation } from "@/features/chat/hooks/useChatQueries";
import { useAppStore } from "@/shared/store/app-store";
import { formatRelativeTime } from "@/shared/lib/time";
import { cn } from "@/shared/lib/cn";
import { Badge } from "@/shared/ui/Badge";
import { getEffectiveSessionStatus } from "@/shared/types/domain";

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

function formatThreadFilterLabel(filter: string) {
  if (filter === "all") {
    return "All threads";
  }
  if (filter === "waiting_approval") {
    return "Awaiting approval";
  }
  return filter.replaceAll("_", " ");
}

export function Sidebar() {
  const location = useLocation();
  const navigate = useNavigate();
  const { data: models } = useModelsQuery();
  const sessionsQuery = useSessionsQuery();
  const createSession = useCreateSessionMutation();
  const closeSession = useCloseSessionMutation();
  const threadFilter = useAppStore((state) => state.threadFilter);
  const setThreadFilter = useAppStore((state) => state.setThreadFilter);
  const aliases = useAppStore((state) => state.sessionAliases);
  const statuses = useAppStore((state) => state.transientStatuses);
  const streams = useAppStore((state) => state.streams);
  const visibleSessions = (sessionsQuery.data ?? []).slice(0, 120);
  const isHomePage = location.pathname === "/";

  return (
    <aside className="sticky top-0 flex h-screen w-full shrink-0 flex-col border-r border-border bg-sidebar px-3 py-3">
      <Link className="mb-4 block rounded-[1.25rem] border border-border/80 bg-card/80 px-3 py-3" to="/">
        <div className="flex items-center justify-between gap-3">
          <div>
            <div className="font-display text-[21px] font-semibold tracking-[-0.04em] text-ink">Talos</div>
            <div className="mt-1 text-[10px] uppercase tracking-[0.22em] text-muted">Web console</div>
          </div>
          <div className="flex size-9 items-center justify-center rounded-2xl border border-brand/30 bg-brand/10 text-brand">
            ⬡
          </div>
        </div>
        <div className="mt-3 flex items-center gap-2 text-[11px] text-muted">
          <span className="size-2 rounded-full bg-success shadow-[0_0_14px_rgba(94,234,212,0.55)]" />
          <span>Connected to server</span>
        </div>
      </Link>

      <div className="mb-3 flex items-end justify-between gap-3">
        <div>
          <div className="text-xs uppercase tracking-[0.18em] text-muted">Threads</div>
          <div className="mt-1 text-[22px] font-display font-semibold tracking-[-0.04em] text-ink">
            {sessionsQuery.data?.length ?? 0}
          </div>
          <div className="mt-1 text-[11px] text-muted">{formatThreadFilterLabel(threadFilter)}</div>
        </div>
        <div className="flex items-center gap-2">
          <button
            className="rounded-2xl border border-border bg-card p-2.5 text-muted transition hover:border-brand/40 hover:text-ink"
            onClick={() =>
              setThreadFilter(
                threadFilter === "all"
                  ? "running"
                  : threadFilter === "running"
                    ? "waiting_approval"
                    : "all",
              )
            }
            type="button"
          >
            <SlidersHorizontal size={16} />
          </button>
          {!isHomePage ? (
            <button
              className="rounded-2xl border border-border bg-card p-2.5 text-muted transition hover:border-brand/40 hover:text-ink"
              onClick={async () => {
                const model = models?.[0]?.id;
                const session = await createSession.mutateAsync(model);
                navigate(`/sessions/${session.context.session_id}`);
              }}
              type="button"
            >
              <Plus size={16} />
            </button>
          ) : null}
        </div>
      </div>

      <div className="min-h-0 flex-1 space-y-2 overflow-y-auto pr-1">
        {visibleSessions.map((session) => {
          const sessionId = session.context.session_id;
          const status = getEffectiveSessionStatus(statuses[sessionId], streams[sessionId]);
          const label = aliases[sessionId] ?? session.title;
          return (
            <div
              className={cn(
                "group cursor-pointer rounded-[1.05rem] border border-transparent bg-transparent px-3 py-2 transition hover:border-border hover:bg-card/90",
                location.pathname === `/sessions/${sessionId}` && "border-border bg-card/95 shadow-panel",
              )}
              onClick={() => navigate(`/sessions/${sessionId}`)}
              key={`${sessionId}-${session.state.created_at}`}
              role="button"
              tabIndex={0}
              onKeyDown={(event) => {
                if (event.key === "Enter" || event.key === " ") {
                  event.preventDefault();
                  navigate(`/sessions/${sessionId}`);
                }
              }}
            >
              <div className="flex items-center justify-between gap-2">
                <span className="line-clamp-2 text-[12.5px] font-medium leading-5 text-ink">{label}</span>
                <Badge tone={statusTone(status)}>{status.replaceAll("_", " ")}</Badge>
              </div>
              <div className="mt-1.5 flex items-center justify-between gap-3 text-[10px] text-muted">
                <span className="truncate">{session.config.llm?.model ?? session.config.agent?.model ?? "unknown"}</span>
                <span>{formatRelativeTime(session.state.last_activity)}</span>
              </div>
              <div className="mt-1.5 flex items-center justify-end text-xs opacity-0 transition group-hover:opacity-100 group-focus-within:opacity-100">
                <button
                  className="inline-flex items-center gap-1 rounded-xl px-2 py-1 text-muted transition hover:bg-danger/10 hover:text-danger"
                  onClick={async (event) => {
                    event.stopPropagation();
                    await closeSession.mutateAsync(sessionId);
                    if (location.pathname === `/sessions/${sessionId}`) {
                      navigate("/");
                    }
                  }}
                  type="button"
                >
                  <Trash2 size={14} />
                  Delete
                </button>
              </div>
            </div>
          );
        })}
      </div>

      <nav className="mt-5 space-y-1 border-t border-border pt-5">
        <NavItem icon={<MemoryStick size={16} />} label="Memory" to="/memory" />
        <NavItem icon={<HardDriveUpload size={16} />} label="Files" to="/files" />
        <NavItem icon={<Settings2 size={16} />} label="Settings" to="/settings" />
        <NavItem icon={<Shield size={16} />} label="Admin" to="/admin" />
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
          "flex items-center gap-3 rounded-md px-3 py-2.5 text-sm text-muted transition hover:bg-card hover:text-ink",
          isActive && "bg-card text-ink",
        )
      }
      to={to}
    >
      {icon}
      <span>{label}</span>
    </NavLink>
  );
}
