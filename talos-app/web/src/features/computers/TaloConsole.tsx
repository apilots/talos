import { useCallback, useEffect, useRef, useState, type MouseEvent, type ReactNode, type RefObject } from "react";
import { useQuery } from "@tanstack/react-query";
import type { AppProtocol } from "@talos/sdk-core";
import {
  ChevronRight,
  CircleHelp,
  EllipsisVertical,
  Globe,
  Loader2,
  Monitor,
  Moon,
  Play,
  Power,
  RotateCcw,
  SlidersHorizontal,
  Trash2,
  X,
} from "lucide-react";
import { useTranslation } from "react-i18next";

import { useCanUseTalo } from "@/features/auth";
import { snapshotTasks } from "@/features/automation/model";
import { SettingsModelsSection } from "@/features/settings";
import { getTalosClient } from "@/shared/api/client";
import { cn } from "@/shared/lib/cn";
import { Button } from "@/shared/ui/Button";
import { Dialog, DialogClose, DialogContent, DialogTitle } from "@/shared/ui/Dialog";
import { Panel } from "@/shared/ui/Panel";
import { ComputerBrowserConfiguration } from "./ComputerBrowserConfiguration";
import { useProductScope } from "./hooks";
import { TaloConsoleFilesTab } from "./TaloConsoleFilesTab";
import { TaloDesktopDialog } from "./TaloDesktopDialog";
import { TaloHoverTip } from "./TaloHoverTip";
import {
  TaloConfirmationDialog,
  TaloEmblem,
  formatTaloStatusLabel,
  taloDotTone,
  taloPendingPhases,
  useTaloController,
} from "./talo-lifecycle";

const GIB = 1024 ** 3;
const rowFocusClass = "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-brand/40";

type ConsoleTab = "console" | "files";

function TaloActionRow({
  actionRef,
  disabled,
  danger,
  chevron = true,
  icon,
  label,
  tip,
  tipAriaLabel,
  onClick,
}: {
  actionRef?: RefObject<HTMLButtonElement | null>;
  disabled?: boolean;
  danger?: boolean;
  chevron?: boolean;
  icon: ReactNode;
  label: string;
  tip: string;
  tipAriaLabel: string;
  onClick: (event: MouseEvent<HTMLButtonElement>) => void;
}) {
  return (
    <div className="group flex items-center hover:bg-hover">
      <button
        ref={actionRef}
        type="button"
        disabled={disabled}
        onClick={onClick}
        className={cn(
          "flex min-w-0 flex-1 items-center gap-2.5 py-2.5 pl-3.5 pr-1 text-left font-medium transition disabled:cursor-not-allowed disabled:opacity-50",
          danger ? "text-danger" : "text-ink",
          rowFocusClass,
        )}
      >
        {icon}
        <span className="truncate">{label}</span>
      </button>
      <div className="flex shrink-0 items-center gap-1 pr-3">
        <TaloHoverTip content={tip}>
          <button
            type="button"
            aria-label={tipAriaLabel}
            className="flex size-5 items-center justify-center rounded-full text-muted/35 transition group-hover:text-muted/80 hover:!text-ink focus-visible:text-ink focus-visible:outline focus-visible:outline-2 focus-visible:outline-brand"
          >
            <CircleHelp size={13} aria-hidden="true" />
          </button>
        </TaloHoverTip>
        {chevron ? (
          <ChevronRight
            size={14}
            aria-hidden="true"
            className={cn("pointer-events-none shrink-0", danger ? "text-danger/70" : "text-muted")}
          />
        ) : null}
      </div>
    </div>
  );
}

function phaseBadgeTone(phase: AppProtocol.ManagedWorkerPhase | undefined) {
  if (phase === "ready") return "border-success/30 bg-success/10 text-success";
  if (phase === "sleeping") return "border-warning/30 bg-warning/10 text-warning";
  if (phase && taloPendingPhases.has(phase)) return "border-accent/30 bg-accent/10 text-accent";
  if (phase === "failed") return "border-danger/30 bg-danger/10 text-danger";
  return "border-border bg-surface text-muted";
}

function formatIdleTimeout(seconds: number | undefined, language: string) {
  if (seconds == null || seconds <= 0) return null;
  const zh = language.toLowerCase().startsWith("zh");
  if (seconds % 3600 === 0) {
    const hours = seconds / 3600;
    return zh ? `${hours} 小时` : `${hours}h`;
  }
  if (seconds % 60 === 0) {
    const minutes = seconds / 60;
    return zh ? `${minutes} 分钟` : `${minutes}m`;
  }
  return zh ? `${seconds} 秒` : `${seconds}s`;
}

export type TaloConsoleActionRequest = {
  id: number;
  kind: "models" | "files";
};

export function TaloConsole({
  initialInstance,
  sessionId,
  actionRequest,
}: {
  initialInstance?: AppProtocol.ManagedWorkerInfo;
  sessionId?: string;
  actionRequest?: TaloConsoleActionRequest | null;
}) {
  const canUseTalo = useCanUseTalo();
  if (!canUseTalo) return null;
  return <AuthorizedTaloConsole initialInstance={initialInstance} sessionId={sessionId} actionRequest={actionRequest} />;
}

function AuthorizedTaloConsole({
  initialInstance,
  sessionId,
  actionRequest,
}: {
  initialInstance?: AppProtocol.ManagedWorkerInfo;
  sessionId?: string;
  actionRequest?: TaloConsoleActionRequest | null;
}) {
  const { t, i18n } = useTranslation(["computers", "common"]);
  const scope = useProductScope();
  const [open, setOpen] = useState(false);
  const [tab, setTab] = useState<ConsoleTab>("console");
  const [desktopOpen, setDesktopOpen] = useState(false);
  const [modelConfigOpen, setModelConfigOpen] = useState(false);
  const [browserConfigOpen, setBrowserConfigOpen] = useState(false);
  const panelRef = useRef<HTMLElement>(null);
  const closeButtonRef = useRef<HTMLButtonElement>(null);
  const emblemTriggerRef = useRef<HTMLButtonElement>(null);
  const dockTriggerRef = useRef<HTMLButtonElement>(null);
  const openerKindRef = useRef<"emblem" | "dock">("dock");
  const confirmTriggerRef = useRef<HTMLElement | null>(null);
  const desktopLauncherRef = useRef<HTMLElement | null>(null);
  const modelConfigTriggerRef = useRef<HTMLButtonElement>(null);
  const browserConfigTriggerRef = useRef<HTMLButtonElement>(null);
  const handledActionIdRef = useRef<number | null>(null);

  const {
    instance,
    phase,
    operation,
    busy,
    desktopPending,
    error,
    confirmation,
    setConfirmation,
    perform,
    connectDesktop,
  } = useTaloController(initialInstance, () => setDesktopOpen(true));

  const automationSessionId = sessionId ?? instance?.conversationSessionId ?? undefined;
  const automationQuery = useQuery({
    enabled: Boolean(open && tab === "console" && phase === "ready" && automationSessionId),
    queryKey: ["talos", scope, "automation", automationSessionId ?? ""],
    queryFn: async () =>
      (await getTalosClient({ sessionId: automationSessionId!, readOnly: true })).automation.read({
        sessionId: automationSessionId!,
      }),
    retry: false,
    refetchOnWindowFocus: false,
  });

  const closeConsole = useCallback(() => {
    const active = document.activeElement;
    const restoreFocus = active === document.body || (active instanceof Node && panelRef.current?.contains(active));
    setOpen(false);
    if (restoreFocus) {
      requestAnimationFrame(() => {
        const target = openerKindRef.current === "emblem" ? emblemTriggerRef.current : dockTriggerRef.current;
        (target ?? dockTriggerRef.current)?.focus();
      });
    }
  }, []);

  useEffect(() => {
    if (!actionRequest || handledActionIdRef.current === actionRequest.id) return;
    handledActionIdRef.current = actionRequest.id;
    if (actionRequest.kind === "models") {
      if (phase === "ready") {
        setModelConfigOpen(true);
      } else {
        openerKindRef.current = "dock";
        setTab("console");
        setOpen(true);
      }
    } else {
      openerKindRef.current = "dock";
      setTab("files");
      setOpen(true);
    }
  }, [actionRequest, phase]);

  useEffect(() => {
    if (!open || confirmation || desktopOpen || modelConfigOpen || browserConfigOpen) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || event.defaultPrevented) return;
      const panel = panelRef.current;
      if (!panel || panel.closest("[inert]")) return;
      const active = document.activeElement;
      if (
        active instanceof HTMLElement &&
        active !== document.body &&
        !panel.contains(active) &&
        active.closest("[role='dialog'], [role='alertdialog'], [role='menu'], [role='listbox'], dialog[open]")
      ) {
        return;
      }
      event.stopPropagation();
      closeConsole();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [browserConfigOpen, closeConsole, confirmation, desktopOpen, modelConfigOpen, open]);

  if (!instance) return null;

  const transitioning = phase !== undefined && taloPendingPhases.has(phase);
  const desktopDisabled = desktopPending || phase === "deleting";
  const canStop = phase === "ready" || phase === "sleeping" || (phase === "failed" && operation === "stop");
  const canStart = phase === "stopped" || phase === "sleeping" || phase === "offline" ||
    (phase === "failed" && operation !== "stop" && operation !== "delete");
  const primaryPowerAction = phase === "ready" ? "stop" : canStart ? "start" : canStop ? "stop" : null;
  const startActionLabel = phase === "sleeping"
    ? t("computers:talo.wake")
    : phase === "failed" || phase === "offline"
      ? t("computers:talo.retryStart")
      : t("computers:talo.start");
  const stopActionLabel = t(phase === "failed" && operation === "stop" ? "computers:talo.retryStop" : "computers:talo.stop");
  const powerLabel = primaryPowerAction === "stop" ? stopActionLabel : startActionLabel;

  const phaseLabel = formatTaloStatusLabel((key) => t(`computers:${key}`), phase);
  const desktopSubtitle =
    phase === "ready"
      ? t("computers:talo.desktopSubtitleReady")
      : phase === "sleeping"
        ? t("computers:talo.desktopSubtitleSleeping")
        : phase === "stopped"
          ? t("computers:talo.desktopSubtitleStopped")
          : phaseLabel;
  const idleDuration = formatIdleTimeout(instance.idleTimeoutSeconds, i18n.language);
  const sleepBadgeLabel = idleDuration
    ? t("computers:talo.autoSleepBadgeWithDuration", { duration: idleDuration })
    : t("computers:talo.autoSleepBadge");
  const sleepTipText = idleDuration
    ? t("computers:talo.sleepTip", { duration: idleDuration })
    : t("computers:talo.sleepTipNoDuration");
  const activeAutomationCount = snapshotTasks(automationQuery.data).length;
  const automationLabel =
    phase !== "ready" || !automationSessionId || automationQuery.error
      ? t("computers:talo.automationUnavailable")
      : automationQuery.isLoading
        ? "…"
        : activeAutomationCount > 0
          ? t("computers:talo.automationRunning", { count: activeAutomationCount })
          : t("computers:talo.automationNone");
  const memoryGiB = Math.max(1, Math.round((instance.memoryBytes / GIB) * 10) / 10);
  const dotClass = taloDotTone(phase);

  const openConsole = (kind: "emblem" | "dock") => {
    openerKindRef.current = kind;
    setOpen(true);
    requestAnimationFrame(() => {
      closeButtonRef.current?.focus();
    });
  };

  const requestDesktopFrom = (element: HTMLElement) => {
    desktopLauncherRef.current = element;
    connectDesktop();
  };

  return (
    <>
      {!open ? (
        <>
          {/* Top-center floating Talo emblem (Variant B collapsed state) */}
          <div className="pointer-events-none absolute inset-x-0 top-3 z-20 flex justify-center px-16">
            <button
              ref={emblemTriggerRef}
              type="button"
              onClick={() => openConsole("emblem")}
              aria-label={`${t("computers:talo.title")} · ${phaseLabel}`}
              aria-expanded={open}
              className="pointer-events-auto group flex flex-col items-center gap-1 rounded-2xl px-2.5 py-1 transition hover:-translate-y-0.5 focus-visible:outline focus-visible:outline-2 focus-visible:outline-brand"
            >
              <span className="relative flex size-10 items-center justify-center rounded-2xl border border-border bg-card/95 shadow-panel backdrop-blur-md">
                <TaloEmblem size={26} />
                <span
                  aria-hidden="true"
                  className={cn("absolute -bottom-0.5 -right-0.5 size-2.5 rounded-full ring-2 ring-background", dotClass)}
                />
              </span>
              <span className="inline-flex items-center gap-1.5 rounded-full border border-border bg-card/95 px-2.5 py-0.5 text-xs font-semibold text-ink shadow-sm backdrop-blur-md">
                {t("computers:talo.title")}
              </span>
            </button>
          </div>

          {/* Right-edge compact floating menu (vertical 3-dots, never conflicts with RightWorkspace) */}
          <div
            role="group"
            aria-label={t("computers:talo.manage")}
            className="absolute right-3.5 top-3 z-20 flex flex-col items-center gap-1 rounded-2xl border border-border bg-card/95 p-1.5 shadow-panel backdrop-blur-md"
          >
            <Button
              ref={dockTriggerRef}
              type="button"
              variant="ghost"
              aria-label={t("computers:talo.openConsole")}
              title={t("computers:talo.openConsole")}
              aria-expanded={open}
              onClick={() => openConsole("dock")}
              className="size-8 rounded-xl p-0 text-muted hover:text-ink"
            >
              <EllipsisVertical size={16} aria-hidden="true" />
            </Button>
            <span aria-hidden="true" className="my-0.5 h-px w-5 bg-divider" />
            <Button
              type="button"
              variant="ghost"
              disabled={desktopDisabled}
              aria-label={t("computers:talo.connect")}
              title={t("computers:talo.connect")}
              onClick={(event) => requestDesktopFrom(event.currentTarget)}
              className="relative size-8 rounded-xl p-0 text-muted hover:text-ink"
            >
              {desktopPending ? (
                <Loader2 size={15} aria-hidden="true" className="animate-spin motion-reduce:animate-none" />
              ) : (
                <Monitor size={15} aria-hidden="true" />
              )}
              <span aria-hidden="true" className={cn("absolute bottom-1 right-1 size-1.5 rounded-full", dotClass)} />
            </Button>
            {primaryPowerAction ? (
              <Button
                type="button"
                variant="ghost"
                disabled={busy}
                aria-label={powerLabel}
                title={powerLabel}
                onClick={() => perform(primaryPowerAction)}
                className="size-8 rounded-xl p-0 text-muted hover:text-ink"
              >
                {busy ? (
                  <Loader2 size={15} aria-hidden="true" className="animate-spin motion-reduce:animate-none" />
                ) : primaryPowerAction === "stop" ? (
                  <Power size={15} aria-hidden="true" />
                ) : (
                  <Play size={15} aria-hidden="true" />
                )}
              </Button>
            ) : null}
          </div>
        </>
      ) : (
        /* Variant B floating shell + Variant A centered hero & sectioned interior */
        <aside
          ref={panelRef}
          id="talo-floating-console"
          aria-label={t("computers:talo.manage")}
          className="absolute right-3.5 top-3 z-30 flex max-h-[calc(100%-1.5rem)] w-[min(336px,calc(100%-1.75rem))] flex-col overflow-hidden rounded-2xl border border-border bg-card/95 shadow-panel backdrop-blur-md"
        >
          {/* Centered Hero + Top-Right Close Button (Variant A interior) */}
          <div className="relative flex shrink-0 flex-col items-center px-5 pb-3 pt-5 text-center">
            <Button
              ref={closeButtonRef}
              type="button"
              variant="ghost"
              aria-label={t("computers:talo.closeConsole")}
              onClick={closeConsole}
              className="absolute right-3 top-3 size-8 rounded-xl p-0 text-muted hover:text-ink"
            >
              <X size={16} aria-hidden="true" />
            </Button>

            <TaloEmblem size={44} />
            <h2 className="mt-2 text-lg font-bold tracking-tight text-ink">{t("computers:talo.title")}</h2>

            <p
              role="status"
              className={cn(
                "mt-1.5 inline-flex max-w-full items-center gap-1.5 rounded-full border px-3 py-0.5 text-xs font-medium",
                phaseBadgeTone(phase),
              )}
            >
              <span aria-hidden="true" className={cn("size-1.5 shrink-0 rounded-full", dotClass)} />
              <span className="truncate">{phaseLabel}</span>
            </p>

            {/* Segmented Tabs: Console | Files */}
            <div
              role="tablist"
              aria-label={t("computers:talo.manage")}
              className="mt-3 inline-flex rounded-full border border-border bg-background p-1"
              onKeyDown={(event) => {
                if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
                event.preventDefault();
                const next: ConsoleTab = tab === "console" ? "files" : "console";
                setTab(next);
                event.currentTarget.querySelector<HTMLButtonElement>(`#talo-console-tab-${next}`)?.focus();
              }}
            >
              {(["console", "files"] as const).map((entry) => {
                const selected = tab === entry;
                return (
                  <button
                    key={entry}
                    id={`talo-console-tab-${entry}`}
                    type="button"
                    role="tab"
                    aria-selected={selected}
                    aria-controls={`talo-console-panel-${entry}`}
                    tabIndex={selected ? 0 : -1}
                    onClick={() => setTab(entry)}
                    className={cn(
                      "rounded-full px-4 py-1 text-xs transition focus-visible:outline focus-visible:outline-2 focus-visible:outline-brand",
                      selected ? "bg-selected font-semibold text-ink" : "font-medium text-muted hover:text-ink",
                    )}
                  >
                    {t(entry === "console" ? "computers:talo.consoleTab" : "computers:talo.filesTab")}
                  </button>
                );
              })}
            </div>
          </div>

          {tab === "console" ? (
            <div
              id="talo-console-panel-console"
              role="tabpanel"
              aria-labelledby="talo-console-tab-console"
              className="min-h-0 flex-1 space-y-3 overflow-y-auto border-t border-divider px-4 pb-4 pt-3"
            >
              {error ? (
                <p role="alert" className="rounded-xl border border-danger/20 bg-danger/5 px-3.5 py-2.5 text-xs leading-5 text-danger">
                  {error}
                </p>
              ) : null}

              {/* 1. Remote Desktop Section */}
              <section className="space-y-1.5">
                <h3 className="px-1 text-[11px] font-semibold text-muted">{t("computers:talo.remoteDesktop")}</h3>
                <Panel className="overflow-hidden bg-surface/60">
                  <button
                    type="button"
                    disabled={desktopDisabled}
                    onClick={(event) => requestDesktopFrom(event.currentTarget)}
                    className={cn(
                      "flex w-full items-center justify-between gap-3.5 p-3 text-left transition hover:bg-hover disabled:cursor-not-allowed disabled:opacity-70",
                      rowFocusClass,
                    )}
                  >
                    <div className="min-w-0 flex-1">
                      <div className="flex items-center gap-1.5 text-sm font-semibold text-ink">
                        <span className="truncate">{t("computers:talo.desktopTitle")}</span>
                        <ChevronRight size={14} aria-hidden="true" className="shrink-0 text-muted" />
                      </div>
                      <p className="mt-1 truncate text-xs text-muted">{desktopSubtitle}</p>
                    </div>
                    <span
                      aria-hidden="true"
                      className={cn(
                        "relative flex h-11 w-[4.5rem] shrink-0 items-center justify-center overflow-hidden rounded-lg border border-border",
                        phase === "ready" ? "bg-[#1b2029]" : "bg-surface text-muted",
                      )}
                    >
                      {transitioning || desktopPending ? (
                        <Loader2 size={16} className="animate-spin text-accent motion-reduce:animate-none" />
                      ) : phase === "sleeping" ? (
                        <Moon size={16} className="text-warning" />
                      ) : phase === "ready" ? (
                        <>
                          <span className="absolute inset-x-2 bottom-2.5 top-1.5 rounded-[3px] border border-white/15 bg-white/10" />
                          <span className="absolute inset-x-4 bottom-1 h-1 rounded-full bg-emerald-400/80" />
                        </>
                      ) : (
                        <Monitor size={16} />
                      )}
                    </span>
                  </button>
                </Panel>
              </section>

              {/* 2. Lifecycle & Controls Section (tips triggered cleanly via ? icon) */}
              <section className="space-y-1.5">
                <div className="flex items-center justify-between gap-2 px-1">
                  <h3 className="text-[11px] font-semibold text-muted">{t("computers:talo.lifecycleSection")}</h3>
                  <span className="inline-flex items-center gap-1 rounded-full border border-border bg-surface py-0.5 pl-2 pr-1 text-[10px] font-medium text-muted">
                    <Moon size={10} aria-hidden="true" />
                    <span>{sleepBadgeLabel}</span>
                    <TaloHoverTip content={sleepTipText}>
                      <button
                        type="button"
                        aria-label={t("computers:talo.sleepTipLabel")}
                        className="flex size-4 items-center justify-center rounded-full text-muted/60 transition hover:text-ink focus-visible:text-ink focus-visible:outline focus-visible:outline-2 focus-visible:outline-brand"
                      >
                        <CircleHelp size={11} aria-hidden="true" />
                      </button>
                    </TaloHoverTip>
                  </span>
                </div>
                <Panel className="divide-y divide-divider overflow-hidden bg-surface/60 text-xs">
                  {canStart ? (
                    <TaloActionRow
                      disabled={busy}
                      chevron={false}
                      icon={busy && operation === "start" ? (
                        <Loader2 size={14} aria-hidden="true" className="shrink-0 animate-spin text-muted motion-reduce:animate-none" />
                      ) : (
                        <Play size={14} aria-hidden="true" className="shrink-0 text-muted" />
                      )}
                      label={startActionLabel}
                      tip={t("computers:talo.startTip")}
                      tipAriaLabel={t("computers:talo.actionTipLabel", { action: startActionLabel })}
                      onClick={() => perform("start")}
                    />
                  ) : null}
                  {canStop ? (
                    <TaloActionRow
                      disabled={busy}
                      chevron={false}
                      icon={busy && operation === "stop" ? (
                        <Loader2 size={14} aria-hidden="true" className="shrink-0 animate-spin text-muted motion-reduce:animate-none" />
                      ) : (
                        <Power size={14} aria-hidden="true" className="shrink-0 text-muted" />
                      )}
                      label={stopActionLabel}
                      tip={t("computers:talo.stopTip")}
                      tipAriaLabel={t("computers:talo.actionTipLabel", { action: stopActionLabel })}
                      onClick={() => perform("stop")}
                    />
                  ) : null}
                  <TaloActionRow
                    actionRef={modelConfigTriggerRef}
                    disabled={phase !== "ready"}
                    icon={<SlidersHorizontal size={14} aria-hidden="true" className="shrink-0 text-muted" />}
                    label={t("computers:talo.modelConfig")}
                    tip={t("computers:talo.modelConfigTip")}
                    tipAriaLabel={t("computers:talo.actionTipLabel", { action: t("computers:talo.modelConfig") })}
                    onClick={() => setModelConfigOpen(true)}
                  />
                  <TaloActionRow
                    actionRef={browserConfigTriggerRef}
                    disabled={phase !== "ready"}
                    icon={<Globe size={14} aria-hidden="true" className="shrink-0 text-muted" />}
                    label={t("computers:talo.browserConfig")}
                    tip={t("computers:talo.browserConfigTip")}
                    tipAriaLabel={t("computers:talo.actionTipLabel", { action: t("computers:talo.browserConfig") })}
                    onClick={() => setBrowserConfigOpen(true)}
                  />
                  <TaloActionRow
                    disabled={busy || operation === "delete"}
                    icon={<RotateCcw size={14} aria-hidden="true" className="shrink-0 text-muted" />}
                    label={t("computers:talo.resetRuntime")}
                    tip={t("computers:talo.resetTip")}
                    tipAriaLabel={t("computers:talo.actionTipLabel", { action: t("computers:talo.resetRuntime") })}
                    onClick={(event) => {
                      confirmTriggerRef.current = event.currentTarget;
                      setConfirmation({ kind: "reset", instance });
                    }}
                  />
                  <TaloActionRow
                    disabled={busy}
                    danger
                    icon={<Trash2 size={14} aria-hidden="true" className="shrink-0" />}
                    label={t("computers:talo.delete")}
                    tip={t("computers:talo.deleteTip")}
                    tipAriaLabel={t("computers:talo.actionTipLabel", { action: t("computers:talo.delete") })}
                    onClick={(event) => {
                      confirmTriggerRef.current = event.currentTarget;
                      setConfirmation({ kind: "delete", instance });
                    }}
                  />
                </Panel>
              </section>

              {/* 3. Managed Environment Specs */}
              <section className="space-y-1.5">
                <h3 className="px-1 text-[11px] font-semibold text-muted">{t("computers:talo.environmentSection")}</h3>
                <Panel className="divide-y divide-divider bg-surface/60 text-xs">
                  <div className="flex items-center justify-between gap-3 px-3.5 py-2.5">
                    <span className="text-muted">{t("computers:talo.computeSpec")}</span>
                    <span className="font-mono text-ink">
                      {t("computers:talo.computeValue", { cpu: instance.cpuCores, memory: memoryGiB })}
                    </span>
                  </div>
                  <div className="flex items-center justify-between gap-3 px-3.5 py-2.5">
                    <span className="text-muted">{t("computers:talo.storageSpec")}</span>
                    <span className="text-ink">
                      {t(instance.diskQuota === "enforced" ? "computers:talo.storageEnforced" : "computers:talo.storageUnbounded")}
                    </span>
                  </div>
                  <div className="flex items-center justify-between gap-3 px-3.5 py-2.5">
                    <span className="text-muted">{t("computers:talo.automationSpec")}</span>
                    <span className="text-ink">{automationLabel}</span>
                  </div>
                </Panel>
              </section>
            </div>
          ) : (
            <TaloConsoleFilesTab
              active={open && tab === "files"}
              nodeId={instance.nodeId}
              phase={phase}
              phaseLabel={phaseLabel}
            />
          )}
        </aside>
      )}

      <TaloDesktopDialog
        nodeId={instance.nodeId}
        open={desktopOpen}
        returnFocusRef={desktopLauncherRef}
        onOpenChange={setDesktopOpen}
      />

      <Dialog open={modelConfigOpen} onOpenChange={setModelConfigOpen}>
        <DialogContent
          aria-describedby={undefined}
          className="max-h-[88dvh] max-w-[820px] overflow-y-auto"
          onCloseAutoFocus={(event) => {
            if (modelConfigTriggerRef.current?.isConnected && !modelConfigTriggerRef.current.disabled) {
              event.preventDefault();
              modelConfigTriggerRef.current.focus();
            }
          }}
        >
          <div className="mb-4 flex items-center justify-between gap-3">
            <DialogTitle className="text-base font-semibold text-ink">
              {t("computers:talo.modelConfig")}
            </DialogTitle>
            <DialogClose asChild>
              <Button type="button" variant="ghost" className="size-8 p-0" aria-label={t("computers:talo.closeModelConfig")}>
                <X size={16} aria-hidden="true" />
              </Button>
            </DialogClose>
          </div>
          {modelConfigOpen ? <SettingsModelsSection nodeId={instance.nodeId} /> : null}
        </DialogContent>
      </Dialog>

      <Dialog open={browserConfigOpen} onOpenChange={setBrowserConfigOpen}>
        <DialogContent
          aria-describedby={undefined}
          className="max-h-[85dvh] max-w-[600px] overflow-y-auto"
          onCloseAutoFocus={(event) => {
            if (browserConfigTriggerRef.current?.isConnected && !browserConfigTriggerRef.current.disabled) {
              event.preventDefault();
              browserConfigTriggerRef.current.focus();
            }
          }}
        >
          <div className="mb-4 flex items-center justify-between gap-3">
            <DialogTitle className="text-base font-semibold text-ink">
              {t("computers:talo.browserConfig")}
            </DialogTitle>
            <DialogClose asChild>
              <Button type="button" variant="ghost" className="size-8 p-0" aria-label={t("computers:talo.closeBrowserConfig")}>
                <X size={16} aria-hidden="true" />
              </Button>
            </DialogClose>
          </div>
          {browserConfigOpen ? <ComputerBrowserConfiguration nodeId={instance.nodeId} /> : null}
        </DialogContent>
      </Dialog>

      <TaloConfirmationDialog
        confirmation={confirmation}
        returnFocusRef={confirmTriggerRef}
        fallbackFocusRef={closeButtonRef}
        onClose={() => setConfirmation(null)}
        onConfirm={(kind, target) => perform(kind, target)}
      />
    </>
  );
}
