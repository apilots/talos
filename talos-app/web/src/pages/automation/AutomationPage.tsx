import { useRef, useState } from "react";
import { useInfiniteQuery, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { Clock3, Repeat2 } from "lucide-react";
import type { AppProtocol } from "@talos/sdk-browser";
import { getTalosClient } from "@/shared/api/client";
import { useComputers, useProductScope } from "@/features/computers";
import { AutomationListRow } from "@/features/automation";
import { AutomationEditDialog } from "@/features/automation";
import type { AutomationTask } from "@/features/automation";
import { apiErrorMessage, userVisibleErrorMessage } from "@/shared/i18n/apiErrorMessage";
import { Button } from "@/shared/ui/Button";

export function AutomationPage() {
  const scope = useProductScope();
  return <AutomationPageContent key={scope} />;
}

function AutomationPageContent() {
  const { t, i18n } = useTranslation(["automation", "common"]);
  const scope = useProductScope();
  const computers = useComputers();
  const cache = useQueryClient();
  const fallbackFocusRef = useRef<HTMLHeadingElement>(null);
  const [tab, setTab] = useState<"unfinished" | "ended" | "jobs">("unfinished");
  const [editing, setEditing] = useState<AutomationTask>();
  const tasks = useInfiniteQuery({ queryKey: ["talos", scope, "automation", "list", tab],
    initialPageParam: null as string | null,
    queryFn: ({ pageParam }) => getTalosClient().automation.list({ sessionId: null, scope: tab === "ended" ? "ended" : "unfinished", cursor: pageParam, limit: 30 }),
    getNextPageParam: (page) => page.nextCursor ?? undefined, enabled: tab !== "jobs", refetchInterval: 5_000, retry: false });
  const jobs = useQuery({ queryKey: ["talos", scope, "builtin-jobs", computers.selectedId],
    queryFn: () => getTalosClient().builtinJobs.list({ computerId: computers.selectedId! }),
    enabled: tab === "jobs" && Boolean(computers.selectedId), refetchInterval: 5_000, retry: false });
  const mutation = useMutation({
    mutationFn: (request: AppProtocol.AutomationRequest) => getTalosClient().automation.execute(request),
    onSuccess: () => cache.invalidateQueries({ queryKey: ["talos", scope, "automation"] }),
  });
  const jobMutation = useMutation({ mutationFn: (request: AppProtocol.BuiltinJobControlRequest) => getTalosClient().builtinJobs.control(request),
    onSuccess: () => cache.invalidateQueries({ queryKey: ["talos", scope, "builtin-jobs"] }) });
  const entries = tasks.data?.pages.flatMap((page) => page.data) ?? [];
  const error = (tab === "jobs" ? jobs.error ?? jobMutation.error : tasks.error ?? mutation.error);
  const date = (seconds: number | null) => seconds === null ? t("automation:unknown") : new Date(seconds * 1000).toLocaleString(i18n.resolvedLanguage);
  return <div className="h-full min-h-0 overflow-y-auto">
    <div className="mx-auto max-w-[1060px] px-5 py-7 md:px-8">
      <header className="mb-7">
        <h1 ref={fallbackFocusRef} tabIndex={-1} className="flex items-center gap-2 text-xl font-medium tracking-tight text-ink"><Repeat2 size={19} />{t("automation:title")}</h1>
        <p className="mt-2 text-sm text-muted">{t("automation:description")}</p>
      </header>
      <div className="mb-5 flex flex-wrap gap-1 border-b border-border/70 pb-3" role="group" aria-label={t("automation:title")}>
        {(["unfinished", "ended", "jobs"] as const).map((value) => <button key={value} type="button" aria-pressed={tab === value} className={`rounded-lg px-3 py-2 text-sm transition focus-visible:ring-2 focus-visible:ring-brand/25 ${tab === value ? "bg-selected text-ink" : "text-muted hover:bg-hover"}`} onClick={() => { setTab(value); mutation.reset(); jobMutation.reset(); }}>{t(value === "jobs" ? "automation:builtinJobs" : `automation:${value}`)}</button>)}
      </div>
      {error ? <p role="alert" className="mb-4 rounded-xl border border-danger/20 px-4 py-3 text-sm text-danger">{userVisibleErrorMessage(error, "Automation could not be loaded.")}</p> : null}
      {tab !== "jobs" ? <>
        {tasks.isPending ? <p className="py-10 text-sm text-muted">{t("common:loading")}</p> : null}
        {!tasks.isPending && !tasks.error && entries.length === 0 ? <div className="py-16 text-center"><Clock3 size={24} className="mx-auto mb-4 text-muted/60" /><p className="text-sm text-ink">{t("automation:empty")}</p><p className="mt-2 text-xs text-muted">{t("automation:emptyHint")}</p></div> : null}
        <div className="divide-y divide-border/60">
          {entries.map((entry) => {
            const task: AutomationTask = entry.type === "goal" ? { type: "goal", value: entry.goal } : { type: "loop", value: entry.loopTask };
            return <AutomationListRow key={`${task.type}-${task.value.id}`} task={task} pending={mutation.isPending} onEdit={setEditing} onAction={(action) => mutation.mutateAsync({ sessionId: task.value.sessionId, action })} />;
          })}
        </div>
        {tasks.hasNextPage ? <Button variant="ghost" className="mt-4" disabled={tasks.isFetchingNextPage} onClick={() => { void tasks.fetchNextPage(); }}>{t("automation:loadMore")}</Button> : null}
      </> : <>
        <p className="mb-4 text-xs text-muted">{computers.selected?.displayName ?? computers.selectedId ?? t("automation:noComputer")}</p>
        {computers.selectedId && jobs.isPending ? <p className="py-10 text-sm text-muted">{t("common:loading")}</p> : null}
        {jobs.data?.data.length === 0 ? <p className="py-10 text-sm text-muted">{t("automation:noJobs")}</p> : null}
        <div className="space-y-3">{jobs.data?.data.map((job) => <article key={`${job.executionInstanceId}-${job.id}`} className="rounded-xl border border-border/70 px-4 py-4">
          <div className="flex flex-wrap items-start justify-between gap-3">
            <div><h2 className="text-sm font-medium text-ink">{job.name}</h2><p className="mt-1 text-[11px] text-muted">{t("automation:jobScope", { instance: job.executionInstanceId })}</p></div>
            <div className="flex gap-2 text-xs text-muted">{job.running ? <span>{t("automation:running")}</span> : null}<span>{t(job.paused ? "automation:paused" : "automation:scheduled")}</span></div>
          </div>
          <p className="mt-3 text-xs text-muted"><code>{job.schedule}</code> · {job.timezone}</p>
          <p className="mt-1 text-xs text-muted">{t("automation:next")}: {date(job.nextDueAt)} · {t("automation:last")}: {date(job.lastFinishedAt)}</p>
          {job.lastOutcome ? <p className={`mt-1 text-xs ${job.lastOutcome.type === "failed" ? "text-danger" : "text-muted"}`}>{job.lastOutcome.type === "succeeded" ? t("automation:succeeded") : apiErrorMessage(job.lastOutcome.error)}</p> : null}
          <div className="mt-3 flex flex-wrap items-center gap-2">{job.availableActions.map((action) => <Button key={action} variant="secondary" className="px-3 py-1.5 text-xs" disabled={jobMutation.isPending || (action === "runOnce" && job.running)} onClick={() => jobMutation.mutate({ computerId: job.computerId, executionInstanceId: job.executionInstanceId, jobId: job.id, action })}>{t(`automation:${action}`)}</Button>)}<span className="text-[11px] text-muted">{t("automation:scheduleReadonly")}</span></div>
        </article>)}</div>
      </>}
      {editing ? <AutomationEditDialog fallbackFocusRef={fallbackFocusRef} key={`${editing.type}-${editing.value.id}`} task={editing} pending={mutation.isPending} error={mutation.error ? userVisibleErrorMessage(mutation.error, "Automation update failed.") : undefined} onClose={() => { setEditing(undefined); mutation.reset(); }} onSave={(action) => mutation.mutateAsync({ sessionId: editing.value.sessionId, action })} /> : null}
    </div>
  </div>;
}
