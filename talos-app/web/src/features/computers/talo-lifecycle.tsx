import { useCallback, useEffect, useRef, useState, type RefObject } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type { AppProtocol } from "@talos/sdk-core";
import { Ellipsis } from "lucide-react";
import { useTranslation } from "react-i18next";

import { useCanUseTalo } from "@/features/auth";
import { managedWorkerKey } from "@/shared/api/product-queries";
import { getTalosClient } from "@/shared/api/client";
import { apiErrorMessage, userVisibleErrorMessage } from "@/shared/i18n/apiErrorMessage";
import { useAppStore } from "@/shared/store/app-store";
import { connectionScope } from "@/shared/store/connection-scope";
import { workspaceWindowKey } from "@/shared/store/workspace-window";
import { Button } from "@/shared/ui/Button";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from "@/shared/ui/DropdownMenu";
import {
  AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent,
  AlertDialogDescription, AlertDialogTitle,
} from "@/shared/ui/Dialog";
import { cn } from "@/shared/lib/cn";
import { useProductScope } from "./hooks";

export const taloSetupSteps = ["provisioning", "starting", "waitingForRegistration"] as const;
export const taloPendingPhases = new Set<AppProtocol.ManagedWorkerPhase>([
  ...taloSetupSteps, "stopping", "resetting", "deleting",
]);
const pollLimitMs = 120_000;

export type TaloAction = AppProtocol.ManagedWorkerOperation | "connect";
export type TaloConfirmation = { kind: "reset" | "delete"; instance: AppProtocol.ManagedWorkerInfo };

function isFocusableTarget(element: HTMLElement | null | undefined): element is HTMLElement {
  return Boolean(element?.isConnected && !(element as HTMLButtonElement).disabled && !element.closest("[inert], [hidden]"));
}

export function useManagedWorkerStatus(enabled = true) {
  const scope = useProductScope();
  const canUseTalo = useCanUseTalo();
  const queryKey = managedWorkerKey(scope);
  const poll = useRef<{ instanceId: string; generation: number; until: number } | null>(null);
  const status = useQuery({
    enabled: enabled && canUseTalo,
    queryKey,
    queryFn: async () => (await getTalosClient({ service: "server" })).computers.readManagedWorker(),
    retry: false,
    refetchInterval: (query) => {
      const instance = query.state.data?.instance;
      const serviceEnabled = query.state.data?.enabled;
      if (query.state.error) return serviceEnabled ? 5_000 : false;
      if (!serviceEnabled || !instance || !taloPendingPhases.has(instance.phase)) {
        poll.current = null;
        return serviceEnabled ? 5_000 : false;
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

export function formatTaloStatusLabel(
  t: (key: string) => string,
  phase: AppProtocol.ManagedWorkerPhase | undefined,
): string {
  if (!phase) return t("talo.title");
  if (phase === "provisioning") return t("talo.initializing");
  if (phase === "starting") return t("talo.waking");
  return t(`talo.phase.${phase}`);
}

export function useTaloController(fallbackInstance?: AppProtocol.ManagedWorkerInfo, onOpenDesktop?: (nodeId: string) => void) {
  const { t } = useTranslation(["computers", "common"]);
  const scope = useProductScope();
  const queries = useQueryClient();
  const { status, queryKey, poll } = useManagedWorkerStatus();
  const [confirmation, setConfirmation] = useState<TaloConfirmation | null>(null);
  const [desktopIntent, setDesktopIntent] = useState<{ instanceId: string; generation: number }>();

  const instance = status.data ? status.data.instance : fallbackInstance;
  const phase = instance?.phase;
  const operation = instance?.operation;
  const transitioning = phase !== undefined && taloPendingPhases.has(phase);

  const action = useMutation({
    mutationFn: async ({ kind, current }: { kind: TaloAction; current: typeof instance }) => {
      if (scope !== connectionScope(useAppStore.getState().connection)) throw new Error();
      const client = (await getTalosClient({ service: "server" })).computers;
      if (kind === "create") return client.createManagedWorker();
      if (!current) throw new Error();
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
          (target.phase === "ready" || taloPendingPhases.has(target.phase))) {
        setDesktopIntent({ instanceId: target.instanceId, generation: target.generation });
      }
    },
    onError: () => { void queries.invalidateQueries({ queryKey }); },
  });

  useEffect(() => {
    if (transitioning || !status.isSuccess) return;
    void queries.invalidateQueries({ queryKey: ["talos", scope, "computer"] });
  }, [queries, scope, status.isSuccess, instance?.instanceId, instance?.generation, phase, transitioning]);

  useEffect(() => {
    if (desktopIntent && instance?.instanceId === desktopIntent.instanceId &&
        instance.generation === desktopIntent.generation && phase === "ready" && status.isSuccess) {
      setDesktopIntent(undefined);
      onOpenDesktop?.(instance.nodeId);
    }
  }, [desktopIntent, instance?.instanceId, instance?.generation, instance?.nodeId, phase, status.isSuccess, onOpenDesktop]);

  const mutateAction = action.mutate;
  const perform = useCallback((kind: TaloAction, current = instance) => {
    setDesktopIntent(undefined);
    mutateAction({ kind, current });
  }, [mutateAction, instance]);

  const connectDesktop = useCallback(() => {
    if (!instance || phase === "deleting") return;
    if (phase === "ready" || phase === "sleeping") perform("connect");
    else onOpenDesktop?.(instance.nodeId);
  }, [instance, onOpenDesktop, perform, phase]);

  const busy = action.isPending || transitioning;
  const error = action.error ? userVisibleErrorMessage(action.error, t("computers:talo.actionError"))
    : instance?.error ? apiErrorMessage(instance.error) : null;

  return {
    status,
    poll,
    instance,
    phase,
    operation,
    transitioning,
    busy,
    desktopPending: action.isPending || (Boolean(desktopIntent) && transitioning),
    error,
    confirmation,
    setConfirmation,
    perform,
    connectDesktop,
  };
}

export function TaloEmblem({ size = 24, className }: { size?: number; className?: string }) {
  return (
    <svg width={size} height={size} viewBox="0 0 32 32" fill="none" aria-hidden="true" className={cn("shrink-0", className)}>
      <rect width="32" height="32" rx="10" className="fill-ink" />
      <circle cx="16" cy="16" r="7.5" className="stroke-background" strokeWidth="2.2" />
      <circle cx="19" cy="13" r="2.2" className="fill-accent" />
    </svg>
  );
}

export function taloDotTone(phase: AppProtocol.ManagedWorkerPhase | undefined) {
  if (phase === "ready") return "bg-success";
  if (phase === "sleeping") return "bg-warning";
  if (phase && taloPendingPhases.has(phase)) return "bg-accent animate-pulse motion-reduce:animate-none";
  if (phase === "failed") return "bg-danger";
  return "bg-muted/60";
}

export function TaloConfirmationDialog({
  confirmation,
  container,
  returnFocusRef,
  fallbackFocusRef,
  onClose,
  onConfirm,
}: {
  confirmation: TaloConfirmation | null;
  container?: HTMLElement | null;
  returnFocusRef?: RefObject<HTMLElement | null>;
  fallbackFocusRef?: RefObject<HTMLElement | null>;
  onClose: () => void;
  onConfirm: (kind: "reset" | "delete", instance: AppProtocol.ManagedWorkerInfo) => void;
}) {
  const { t } = useTranslation(["computers", "common"]);
  if (!confirmation) return null;
  const isDelete = confirmation.kind === "delete";
  return (
    <AlertDialog open onOpenChange={(open) => { if (!open) onClose(); }}>
      <AlertDialogContent
        container={container}
        onCloseAutoFocus={(event) => {
          const target = isFocusableTarget(returnFocusRef?.current)
            ? returnFocusRef?.current
            : isFocusableTarget(fallbackFocusRef?.current)
              ? fallbackFocusRef?.current
              : null;
          if (target) {
            event.preventDefault();
            target.focus();
          }
        }}
      >
        <AlertDialogTitle className="text-lg font-semibold">
          {t(isDelete ? "computers:talo.confirmDelete" : "computers:talo.confirmReset")}
        </AlertDialogTitle>
        <AlertDialogDescription className="mt-3 space-y-2 text-sm leading-6 text-muted" asChild>
          <div>
            <p>{t(isDelete ? "computers:talo.deleteHelp" : "computers:talo.resetHelp")}</p>
            {isDelete ? <p>{t("computers:talo.retainedHistory")}</p> : null}
          </div>
        </AlertDialogDescription>
        <div className="mt-5 flex justify-end gap-2">
          <AlertDialogCancel asChild>
            <Button type="button" variant="ghost">{t("common:cancel")}</Button>
          </AlertDialogCancel>
          <AlertDialogAction asChild>
            <Button
              type="button"
              variant={isDelete ? "danger" : "primary"}
              onClick={() => onConfirm(confirmation.kind, confirmation.instance)}
            >
              {t(isDelete ? "computers:talo.delete" : "computers:talo.reset")}
            </Button>
          </AlertDialogAction>
        </div>
      </AlertDialogContent>
    </AlertDialog>
  );
}

export function TaloLifecycleMenu({ desktopNodeId, onDeleted }: { desktopNodeId: string; onDeleted?: () => void }) {
  const canUseTalo = useCanUseTalo();
  if (!canUseTalo) return null;
  return <AuthorizedTaloLifecycleMenu desktopNodeId={desktopNodeId} onDeleted={onDeleted} />;
}

function AuthorizedTaloLifecycleMenu({ desktopNodeId, onDeleted }: { desktopNodeId: string; onDeleted?: () => void }) {
  const { t } = useTranslation("computers");
  const menuTrigger = useRef<HTMLButtonElement>(null);
  const [desktopPortal, setDesktopPortal] = useState<HTMLElement | null>(null);
  const { status, instance, phase, operation, busy, error, confirmation, setConfirmation, perform } = useTaloController();

  const assignTriggerRef = useCallback((node: HTMLButtonElement | null) => {
    menuTrigger.current = node;
    setDesktopPortal(node?.closest<HTMLElement>("[role='dialog'], dialog") ?? null);
  }, []);

  useEffect(() => {
    if (status.isSuccess && status.data.instance?.nodeId !== desktopNodeId) {
      useAppStore.getState().closeWorkspaceWindow(workspaceWindowKey({ kind: "computer", nodeId: desktopNodeId }));
      onDeleted?.();
    }
  }, [desktopNodeId, onDeleted, status.data?.instance?.nodeId, status.isSuccess]);

  if (instance?.nodeId !== desktopNodeId) return null;
  const canStop = phase === "ready" || phase === "sleeping";
  const canStart = phase === "stopped" || phase === "sleeping" || phase === "offline" ||
    (phase === "failed" && operation !== "stop" && operation !== "delete");

  return (
    <>
      {error ? <p role="alert" className="max-w-40 truncate text-xs text-danger" title={error}>{error}</p> : null}
      <DropdownMenu modal={false}>
        <DropdownMenuTrigger asChild>
          <Button ref={assignTriggerRef} type="button" variant="ghost" className="p-2" aria-label={t("talo.manage")}>
            <Ellipsis size={16} aria-hidden="true" />
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent container={desktopPortal} align="end">
          {canStart ? <DropdownMenuItem disabled={busy} onSelect={() => perform("start")}>{t("talo.start")}</DropdownMenuItem> : null}
          {canStop || (phase === "failed" && operation === "stop") ? <DropdownMenuItem disabled={busy} onSelect={() => perform("stop")}>{t("talo.stop")}</DropdownMenuItem> : null}
          <DropdownMenuItem disabled={busy || operation === "delete"} onSelect={() => setConfirmation({ kind: "reset", instance })}>{t("talo.reset")}</DropdownMenuItem>
          <DropdownMenuItem disabled={busy} className="text-danger" onSelect={() => setConfirmation({ kind: "delete", instance })}>{t("talo.delete")}</DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
      <TaloConfirmationDialog
        confirmation={confirmation}
        container={desktopPortal}
        returnFocusRef={menuTrigger}
        onClose={() => setConfirmation(null)}
        onConfirm={(kind, target) => perform(kind, target)}
      />
    </>
  );
}
