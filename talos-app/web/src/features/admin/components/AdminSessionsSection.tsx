import { useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useNavigate } from "react-router-dom";
import { useTranslation } from "react-i18next";
import { ArrowUpRight, ChevronLeft, ChevronRight, Trash2 } from "lucide-react";

import type { AdminPageState } from "../hooks/useAdminPage";
import { chatKeys, deleteAdminSession, useConnectionKey } from "@/features/chat";
import { formatRelativeTime } from "@/shared/lib/time";
import { Badge } from "@/shared/ui/Badge";
import { Input } from "@/shared/ui/Input";
import { Panel } from "@/shared/ui/Panel";

export function AdminSessionsSection({
  sessions,
  sessionRows,
  sessionSearch,
  sessionPagination,
  setSessionPage,
  setSessionSearch,
}: Pick<
  AdminPageState,
  "sessions" | "sessionRows" | "sessionSearch" | "sessionPagination" | "setSessionPage" | "setSessionSearch"
>) {
  const { t } = useTranslation(["admin"]);
  const navigate = useNavigate();
  const connectionKey = useConnectionKey();
  const queryClient = useQueryClient();
  const [actionError, setActionError] = useState("");
  const [hiddenSessionIds, setHiddenSessionIds] = useState<string[]>([]);

  function sessionTone(status: "running" | "waiting_approval" | "done" | "error") {
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

  function sessionLabel(status: "running" | "waiting_approval" | "done" | "error") {
    if (status === "waiting_approval") {
      return t("states.needsApproval");
    }
    if (status === "error") {
      return t("states.rejected");
    }
    if (status === "done") {
      return t("states.done");
    }
    return t("states.running");
  }

  function parseErrorMessage(error: unknown): string {
    if (error instanceof Error && "body" in error) {
      const body = String((error as { body?: string }).body ?? "");
      if (body) {
        try {
          const parsed = JSON.parse(body) as { message?: string };
          if (parsed.message) {
            return parsed.message;
          }
        } catch {
          return body;
        }
      }
    }
    if (error instanceof Error && error.message) {
      return error.message;
    }
    return t("sessionsPanel.requestFailed");
  }

  const deleteMutation = useMutation({
    mutationFn: deleteAdminSession,
    onSuccess: async (_, sessionId) => {
      await queryClient.invalidateQueries({ queryKey: ["admin-sessions", connectionKey] });
      await queryClient.invalidateQueries({ queryKey: chatKeys.sessions(connectionKey) });
      queryClient.removeQueries({ queryKey: chatKeys.session(connectionKey, sessionId) });
      queryClient.removeQueries({ queryKey: chatKeys.liveRecovery(connectionKey, sessionId) });
      setActionError("");
    },
    onError: (error) => {
      setActionError(parseErrorMessage(error));
    },
  });

  const visibleSessionRows = sessionRows.filter((row) => !hiddenSessionIds.includes(row.session.context.session_id));

  async function handleDeleteSession(sessionId: string, title: string) {
    setActionError("");
    if (!window.confirm(t("sessionsPanel.deleteConfirm", { title }))) {
      return;
    }
    setHiddenSessionIds((current) => [...current, sessionId]);
    try {
      await deleteMutation.mutateAsync(sessionId);
    } catch {
      setHiddenSessionIds((current) => current.filter((entry) => entry !== sessionId));
      // onError already maps and exposes the failure state for the panel.
    }
  }

  return (
    <Panel className="rounded-[1.4rem] border-border/80 bg-card/82 p-5">
      <div className="mb-4 flex flex-wrap items-center justify-between gap-3">
        <div>
          <div className="text-sm font-medium text-ink">{t("sessionsPanel.title")}</div>
          <div className="mt-1 text-sm text-muted">{t("sessionsPanel.detail")}</div>
        </div>
        <Input
          className="max-w-xs"
          onChange={(event) => setSessionSearch(event.target.value)}
          placeholder={t("sessionsPanel.searchPlaceholder")}
          value={sessionSearch}
        />
      </div>
      {actionError ? (
        <div className="mb-4 rounded-[1rem] border border-danger/20 bg-danger/8 px-4 py-3 text-sm text-danger">
          {t("sessionsPanel.actionErrorPrefix", { message: actionError })}
        </div>
      ) : null}
      <div className="overflow-x-auto">
        <div className="min-w-[620px] space-y-3">
          <div className="grid grid-cols-[96px_minmax(0,1fr)_112px_100px_88px] gap-3 px-4 text-xs uppercase tracking-[0.14em] text-muted">
            <div data-smoke="admin-sessions-status-header">{t("sessionsPanel.status")}</div>
            <div data-smoke="admin-sessions-session-header">{t("sessionsPanel.session")}</div>
            <div data-smoke="admin-sessions-owner-header">{t("sessionsPanel.owner")}</div>
            <div data-smoke="admin-sessions-updated-header">{t("sessionsPanel.updated")}</div>
            <div className="text-right" data-smoke="admin-sessions-actions-header">{t("sessionsPanel.actions")}</div>
          </div>
          {visibleSessionRows.map((row) => (
            <div
              className="grid grid-cols-[96px_minmax(0,1fr)_112px_100px_88px] items-center gap-3 rounded-[1.2rem] border border-border/80 bg-background/45 px-4 py-4"
              key={row.session.context.session_id}
            >
              <Badge tone={sessionTone(row.status)}>{sessionLabel(row.status)}</Badge>
              <div className="min-w-0">
                <div className="truncate text-sm font-medium text-ink">{row.label}</div>
                <div className="mt-1 truncate text-xs text-muted">{row.session.context.session_id}</div>
              </div>
              <div className="min-w-0 truncate text-sm text-muted">{row.ownerLabel}</div>
              <div className="whitespace-nowrap text-sm text-muted">{formatRelativeTime(row.session.state.last_activity)}</div>
              <div className="flex justify-end">
                <div className="inline-flex items-center gap-1.5">
                  <button
                    aria-label={t("sessionsPanel.view")}
                    className="inline-flex h-8 w-8 items-center justify-center rounded-full text-muted transition hover:bg-muted/10 hover:text-ink"
                    onClick={() => navigate(`/sessions/${row.session.context.session_id}`)}
                    title={t("sessionsPanel.view")}
                    type="button"
                  >
                    <ArrowUpRight size={14} />
                  </button>
                  <button
                    aria-label={t("sessionsPanel.delete")}
                    className="inline-flex h-8 w-8 items-center justify-center rounded-full text-danger transition hover:bg-danger/10 hover:text-danger/80 disabled:cursor-not-allowed disabled:text-muted"
                    disabled={deleteMutation.isPending}
                    onClick={() => void handleDeleteSession(row.session.context.session_id, row.label)}
                    title={t("sessionsPanel.delete")}
                    type="button"
                  >
                    <Trash2 size={14} />
                  </button>
                </div>
              </div>
            </div>
          ))}
          {visibleSessionRows.length === 0 ? (
            <div className="rounded-[1.2rem] border border-dashed border-border/80 px-4 py-8 text-sm text-muted">
              {t("sessionsPanel.empty")}
            </div>
          ) : null}
        </div>
      </div>
      {sessionPagination.total > 0 ? (
        <div className="mt-4 flex flex-wrap items-center justify-between gap-3 border-t border-border/70 pt-4">
          <div className="text-sm text-muted">
            {t("sessionsPanel.paginationSummary", {
              from: sessionPagination.from,
              to: sessionPagination.to,
              total: sessionPagination.total,
            })}
            {sessions.isFetching ? (
              <span className="ml-2 text-xs">{t("sessionsPanel.loadingPage")}</span>
            ) : null}
          </div>
          <div className="flex items-center gap-2">
            <div className="text-sm text-muted">
              {t("sessionsPanel.pageLabel", {
                page: sessionPagination.page,
                totalPages: sessionPagination.totalPages,
              })}
            </div>
            <button
              className="inline-flex items-center gap-1 rounded-full border border-border/80 px-3 py-1.5 text-sm text-muted transition hover:border-border hover:text-ink disabled:cursor-not-allowed disabled:opacity-45"
              disabled={!sessionPagination.hasPrevious || sessions.isFetching}
              onClick={() => setSessionPage(sessionPagination.page - 1)}
              type="button"
            >
              <ChevronLeft size={14} />
              {t("sessionsPanel.previousPage")}
            </button>
            <button
              className="inline-flex items-center gap-1 rounded-full border border-border/80 px-3 py-1.5 text-sm text-muted transition hover:border-border hover:text-ink disabled:cursor-not-allowed disabled:opacity-45"
              disabled={!sessionPagination.hasNext || sessions.isFetching}
              onClick={() => setSessionPage(sessionPagination.page + 1)}
              type="button"
            >
              {t("sessionsPanel.nextPage")}
              <ChevronRight size={14} />
            </button>
          </div>
        </div>
      ) : null}
    </Panel>
  );
}
