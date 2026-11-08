import { useCallback, useEffect, useRef, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type { AppProtocol } from "@talos/sdk-core";
import { Bot, Check, Circle, Ellipsis, Loader2 } from "lucide-react";
import { useTranslation } from "react-i18next";

import { getTalosClient } from "@/shared/api/client";
import { apiErrorMessage, userVisibleErrorMessage } from "@/shared/i18n/apiErrorMessage";
import { useAppStore } from "@/shared/store/app-store";
import { workspaceWindowKey } from "@/shared/store/workspace-window";
import { connectionScope } from "@/shared/store/connection-scope";
import { Button } from "@/shared/ui/Button";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from "@/shared/ui/DropdownMenu";
import {
  AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent,
  AlertDialogDescription, AlertDialogTitle,
} from "@/shared/ui/Dialog";
import { useProductScope } from "./hooks";

const setupSteps = ["provisioning", "starting", "waitingForRegistration"] as const;
const pendingPhases = new Set<AppProtocol.ManagedWorkerPhase>([
  ...setupSteps, "stopping", "resetting", "deleting",
]);
const pollLimitMs = 120_000;

type Action = AppProtocol.ManagedWorkerOperation | "connect";
type Confirmation = { kind: "reset" | "delete"; instance: AppProtocol.ManagedWorkerInfo };

export function useManagedWorkerStatus(enabled = true) {
  const scope = useProductScope();
  const queryKey = ["talos", scope, "managed-worker"];
  const poll = useRef<{ instanceId: string; generation: number; until: number } | null>(null);
  const status = useQuery({
    enabled,
    queryKey,
    queryFn: () => getTalosClient().computers.readManagedWorker(),
    retry: false,
    refetchInterval: (query) => {
      const instance = query.state.data?.instance;
      const enabled = query.state.data?.enabled;
      if (query.state.error) return enabled ? 5_000 : false;
      if (!enabled || !instance || !pendingPhases.has(instance.phase)) {
        poll.current = null;
        return enabled ? 5_000 : false;
      }
      if (poll.current?.instanceId !== instance.instanceId || poll.current.generation !== instance.generation) {
        poll.current = { instanceId: instance.instanceId, generation: instance.generation, until: Date.now() + pollLimitMs };
      }
      const remaining = poll.current.until - Date.now();
      return remaining > 0 ? Math.min(2_000, remaining) : 5_000;
    },
  });
  return { status, queryKey, poll };
}

export function taloHasConversation(instance: AppProtocol.ManagedWorkerInfo | null | undefined):
  instance is AppProtocol.ManagedWorkerInfo & { conversationSessionId: string } {
  return Boolean(instance?.conversationSessionId && (instance.operation !== "create" || instance.phase === "failed"));
}

export function TaloPanel({ desktopNodeId, desktopButton = false }: {
  desktopNodeId?: string;
  desktopButton?: boolean;
} = {}) {
  const { t } = useTranslation(["computers", "common"]);
  const scope = useProductScope();
  const queries = useQueryClient();
  const { status, queryKey, poll } = useManagedWorkerStatus();
  const [confirmation, setConfirmation] = useState<Confirmation | null>(null);
  const [desktopIntent, setDesktopIntent] = useState<{ instanceId: string; generation: number }>();
  const menuTrigger = useRef<HTMLButtonElement>(null);
  const [desktopPortal, setDesktopPortal] = useState<HTMLElement | null>(null);

  const instance = status.data?.instance;
  const phase = instance?.phase;
  const operation = instance?.operation;
  const transitioning = phase !== undefined && pendingPhases.has(phase);
  const stepIndex = setupSteps.findIndex(step => step === phase);
  const pollExpired = transitioning && poll.current !== null && Date.now() >= poll.current.until;
  const action = useMutation({
    mutationFn: async ({ kind, current }: { kind: Action; current: typeof instance }) => {
      if (scope !== connectionScope(useAppStore.getState().connection)) throw new Error(t("computers:talo.actionError"));
      const client = getTalosClient().computers;
      if (kind === "create") return client.createManagedWorker();
      if (!current) throw new Error(t("computers:talo.actionError"));
      const params = { instanceId: current.instanceId, expectedGeneration: current.generation };
      switch (kind) {
        case "connect": {
          const latest = await client.readManagedWorker();
          const target = latest.instance;
          if (target?.instanceId === current.instanceId && target.phase === "sleeping" &&
              scope === connectionScope(useAppStore.getState().connection)) {
            return client.startManagedWorker({ instanceId: target.instanceId, expectedGeneration: target.generation });
          }
          return latest;
        }
        case "start": return client.startManagedWorker(params);
        case "stop": return client.stopManagedWorker(params);
        case "reset": return client.resetManagedWorker(params);
        case "delete": return client.deleteManagedWorker({ ...params, confirmDataLoss: true });
      }
    },
    onMutate: () => queries.cancelQueries({ queryKey }),
    onSuccess: (response, { kind, current }) => {
      queries.setQueryData(queryKey, response);
      const target = response.instance;
      if (kind === "connect" && target && target.instanceId === current?.instanceId &&
          (target.phase === "ready" || pendingPhases.has(target.phase))) {
        setDesktopIntent({ instanceId: target.instanceId, generation: target.generation });
      }
    },
    onError: () => { void queries.invalidateQueries({ queryKey }); },
  });
  const busy = action.isPending || transitioning;

  useEffect(() => {
    if (transitioning || !status.isSuccess) return;
    void queries.invalidateQueries({ queryKey: ["talos", scope, "computers"] });
    void queries.invalidateQueries({ queryKey: ["talos", scope, "computer"] });
  }, [queries, scope, status.isSuccess, instance?.instanceId, instance?.generation, phase, transitioning]);

  const openDesktop = useCallback((nodeId: string) => {
    const store = useAppStore.getState();
    if (scope !== connectionScope(store.connection)) return;
    store.selectComputer(scope, nodeId);
    store.openWorkspaceWindow({ kind: "computer", nodeId });
  }, [scope]);

  useEffect(() => {
    if (desktopIntent && instance?.instanceId === desktopIntent.instanceId &&
        instance.generation === desktopIntent.generation && phase === "ready" && status.isSuccess) {
      setDesktopIntent(undefined);
      openDesktop(instance.nodeId);
    }
  }, [desktopIntent, instance?.instanceId, instance?.generation, instance?.nodeId, phase, status.isSuccess, openDesktop]);

  function perform(kind: Action, current = instance) {
    setDesktopIntent(undefined);
    action.mutate({ kind, current });
  }

  useEffect(() => {
    if (desktopNodeId && status.isSuccess && status.data.instance?.nodeId !== desktopNodeId) {
      useAppStore.getState().closeWorkspaceWindow(workspaceWindowKey({ kind: "computer", nodeId: desktopNodeId }));
    }
  }, [desktopNodeId, status.isSuccess, status.data?.instance?.nodeId]);

  const error = action.error ? userVisibleErrorMessage(action.error, t("computers:talo.actionError"))
    : instance?.error ? apiErrorMessage(instance.error) : null;
  const dialog = confirmation ? (
        <AlertDialog open onOpenChange={open => { if (!open) setConfirmation(null); }}>
          <AlertDialogContent container={desktopPortal} onCloseAutoFocus={(event) => {
            event.preventDefault();
            menuTrigger.current?.focus();
          }}>
            <AlertDialogTitle className="text-lg font-semibold">{t(confirmation.kind === "delete" ? "computers:talo.confirmDelete" : "computers:talo.confirmReset")}</AlertDialogTitle>
            <AlertDialogDescription className="mt-3 space-y-2 text-sm leading-6 text-muted" asChild>
              <div>
                <p>{t(confirmation.kind === "delete" ? "computers:talo.deleteHelp" : "computers:talo.resetHelp")}</p>
                {confirmation.kind === "delete" ? <p>{t("computers:talo.retainedHistory")}</p> : null}
              </div>
            </AlertDialogDescription>
            <div className="mt-5 flex justify-end gap-2">
              <AlertDialogCancel asChild><Button type="button" variant="ghost">{t("common:cancel")}</Button></AlertDialogCancel>
              <AlertDialogAction asChild><Button type="button" variant={confirmation.kind === "delete" ? "danger" : "primary"} onClick={() => perform(confirmation.kind, confirmation.instance)}>
                {t(confirmation.kind === "delete" ? "computers:talo.delete" : "computers:talo.reset")}
              </Button></AlertDialogAction>
            </div>
          </AlertDialogContent>
        </AlertDialog>
      ) : null;

  if (desktopNodeId) {
    if (instance?.nodeId !== desktopNodeId) return null;
    const canStop = phase === "ready" || phase === "sleeping";
    const canStart = phase === "stopped" || phase === "sleeping" || phase === "offline" ||
      (phase === "failed" && (operation === "create" || operation === "start"));
    return <>
      {error ? <p role="alert" className="max-w-40 truncate text-xs text-danger" title={error}>{error}</p> : null}
      <DropdownMenu modal={false} onOpenChange={open => {
        if (open) setDesktopPortal(menuTrigger.current?.closest("dialog") ?? null);
      }}>
        <DropdownMenuTrigger asChild>
          <Button ref={menuTrigger} type="button" variant="ghost" className="p-2" aria-label={t("computers:talo.manage")}>
            <Ellipsis size={16} aria-hidden="true" />
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent container={desktopPortal} align="end">
          {canStart ? <DropdownMenuItem disabled={busy} onSelect={() => perform("start")}>{t("computers:talo.start")}</DropdownMenuItem> : null}
          {canStop || (phase === "failed" && operation === "stop") ? <DropdownMenuItem disabled={busy} onSelect={() => perform("stop")}>{t("computers:talo.stop")}</DropdownMenuItem> : null}
          <DropdownMenuItem disabled={busy || operation === "delete"} onSelect={() => setConfirmation({ kind: "reset", instance })}>{t("computers:talo.reset")}</DropdownMenuItem>
          <DropdownMenuItem disabled={busy} className="text-danger" onSelect={() => setConfirmation({ kind: "delete", instance })}>{t("computers:talo.delete")}</DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
      {dialog}
    </>;
  }

  if (desktopButton) {
    if (!instance) return null;
    return <div className="flex items-center gap-2">
      {error ? <p role="alert" className="text-xs text-danger">{error}</p> : null}
      <Button type="button" variant="ghost" disabled={action.isPending || (Boolean(desktopIntent) && transitioning)}
        onClick={() => {
          if (phase === "ready" || phase === "sleeping") perform("connect");
          else openDesktop(instance.nodeId);
        }}>
        {t("computers:talo.connect")}
      </Button>
    </div>;
  }

  if (status.isPending) return <p role="status" className="py-6 text-sm text-muted">{t("common:loading")}</p>;
  if (status.error) return (
    <div className="py-6">
      <p role="alert" className="text-sm text-danger">{userVisibleErrorMessage(status.error, t("computers:talo.loadError"))}</p>
      <Button type="button" variant="ghost" className="mt-2" onClick={() => void status.refetch()}>{t("common:retryNow")}</Button>
    </div>
  );
  if (!status.data.enabled) return <p className="py-6 text-sm text-muted">{t("computers:talo.unavailable")}</p>;

  const help = stepIndex !== -1 ? "pendingHelp" : phase === "ready" ? "readyHelp"
    : phase === "sleeping" ? "sleepingHelp" : phase === "stopped" ? "stoppedHelp"
      : phase === "offline" ? "offlineHelp" : phase ? null : "initializeHelp";
  const title = stepIndex !== -1 ? "initializing" : phase ? `phase.${phase}` as const : "initialize";
  const failedDelete = phase === "failed" && operation === "delete";
  const primary = !instance ? { label: "initialize", run: () => perform("create") }
    : phase === "ready" || phase === "sleeping" ? { label: "connect", run: () => perform("connect") }
    : phase === "failed" && operation === "stop" ? { label: "retryStop", run: () => perform("stop") }
    : failedDelete ? null
    : phase === "stopped" ? { label: "start", run: () => perform("start") }
    : phase === "offline" || phase === "failed" ? { label: "retryStart", run: () => perform("start") } : null;

  return (
    <>
      <div className="flex items-start gap-3 py-6">
        <span className="flex size-11 shrink-0 items-center justify-center rounded-xl border border-border bg-surface text-accent">
          <Bot size={22} aria-hidden="true" />
        </span>
        <div className="min-w-0" role="status">
          <h2 className="text-base font-semibold text-ink">{t(`computers:talo.${title}`)}</h2>
          {help ? <p className="mt-1 text-sm leading-6 text-muted">{t(`computers:talo.${help}`)}</p> : null}
        </div>
      </div>

      {stepIndex !== -1 ? (
        <ol className="divide-y divide-border rounded-xl border border-border bg-surface px-3">
          {setupSteps.map((step, index) => (
            <li key={step} aria-current={index === stepIndex ? "step" : undefined}
              className="flex min-h-10 items-center gap-2.5 text-sm text-muted aria-[current=step]:font-medium aria-[current=step]:text-ink">
              {index < stepIndex ? <Check size={15} aria-hidden="true" className="text-success" />
                : index === stepIndex ? <Loader2 size={15} aria-hidden="true" className="animate-spin text-accent motion-reduce:animate-none" />
                : <Circle size={15} aria-hidden="true" />}
              {t(`computers:talo.phase.${step}`)}
            </li>
          ))}
        </ol>
      ) : null}

      {error ? (
        <p role="alert" className="mb-3 rounded-lg border border-danger/20 bg-danger/5 px-3 py-2 text-sm text-danger">
          {error}
        </p>
      ) : null}

      <div className="mt-3 flex flex-wrap items-start justify-between gap-3 border-t border-border pt-4">
        {!instance ? <p className="my-auto text-xs text-muted">{t("computers:talo.accountLimit")}</p> : null}
        <div className="ml-auto flex flex-col items-end gap-2">
          {pollExpired ? <Button type="button" variant="ghost" onClick={() => {
            poll.current = null;
            void status.refetch();
          }}>{t("computers:talo.refresh")}</Button> : null}
          {primary || busy ? <Button type="button" disabled={busy} onClick={primary?.run}>
            {busy ? t("computers:talo.working") : primary ? t(`computers:talo.${primary.label}`) : null}
          </Button> : null}
          {phase === "ready" && instance?.diskQuota === "unsupported" ? <p className="max-w-56 text-right text-xs leading-5 text-muted">{t("computers:talo.quotaUnsupported")}</p> : null}
        </div>
      </div>
    </>
  );
}
