import { useEffect, useRef, useState } from "react";
import { ArrowUp, Check, ChevronDown, FolderOpen, LoaderCircle, Plus, Search, Square, Zap } from "lucide-react";
import { useTranslation } from "react-i18next";

import { cn } from "@/shared/lib/cn";
import { Panel } from "@/shared/ui/Panel";
import { Textarea } from "@/shared/ui/Input";
import { describeProjectDirectory, isUnboundProject } from "@/shared/types/domain";
import type { ApprovalMode, ProjectRecord, ReasoningLevel, RuntimeMode } from "@/shared/types/ui";

interface ChatComposerProps {
  draft: string;
  onDraftChange: (value: string) => void;
  onSend: () => Promise<void> | void;
  onCancel?: () => void;
  disabled?: boolean;
  isStreaming?: boolean;
  isCancelling?: boolean;
  models: Array<{ id: string; name: string }>;
  selectedModel?: string;
  onModelChange?: (model: string) => void;
  projects: ProjectRecord[];
  activeProjectId: string;
  onProjectChange: (projectId: string) => void;
  onCreateProject?: (path: string) => Promise<void>;
  runtimeMode: RuntimeMode;
  onRuntimeModeChange: (mode: RuntimeMode) => void;
  approvalMode: ApprovalMode;
  onApprovalModeChange: (mode: ApprovalMode) => void;
  reasoningLevel?: ReasoningLevel;
  onReasoningLevelChange?: (level: ReasoningLevel) => void;
  showProjectSelector?: boolean;
  compact?: boolean;
}

// ─── small floating dropdown ────────────────────────────────────────────────

interface DropdownItem {
  value: string;
  label: string;
  icon?: React.ReactNode;
}

interface ComposerDropdownProps {
  header: string;
  items: DropdownItem[];
  value: string | undefined;
  onChange: (v: string) => void;
  triggerIcon?: React.ReactNode;
  className?: string;
}

function ComposerDropdown({ header, items, value, onChange, triggerIcon, className }: ComposerDropdownProps) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  const selectedItem = items.find((i) => i.value === value);

  useEffect(() => {
    if (!open) return;
    function onPointerDown(e: PointerEvent) {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    }
    document.addEventListener("pointerdown", onPointerDown);
    return () => document.removeEventListener("pointerdown", onPointerDown);
  }, [open]);

  return (
    <div className={cn("relative", className)} ref={ref}>
      <button
        className="inline-flex items-center gap-1 rounded-full px-2 py-1 text-[12px] font-medium text-ink/70 outline-none transition hover:bg-muted/15 focus:outline-none"
        onClick={() => setOpen((v) => !v)}
        type="button"
      >
        {triggerIcon}
        <span className="max-w-[120px] truncate">{selectedItem?.label ?? value}</span>
        <ChevronDown size={11} className="shrink-0 text-muted" />
      </button>

      {open && (
        <div className="absolute bottom-[calc(100%+6px)] left-0 z-50 min-w-[160px] overflow-hidden rounded-xl border border-border/60 bg-card py-1 shadow-lg">
          <div className="px-3 pb-1 pt-1.5 text-[11px] font-medium text-muted">{header}</div>
          {items.map((item) => (
            <button
              className="flex w-full items-center gap-2 px-3 py-2 text-left text-[13px] text-ink transition hover:bg-muted/10 focus:outline-none"
              key={item.value}
              onClick={() => {
                onChange(item.value);
                setOpen(false);
              }}
              type="button"
            >
              {item.icon && <span className="shrink-0 text-muted">{item.icon}</span>}
              <span className="flex-1">{item.label}</span>
              {item.value === value && <Check size={12} className="shrink-0 text-brand" />}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

// ─── inline text select (no border) ─────────────────────────────────────────

function InlineSelect({
  value,
  onChange,
  children,
  className,
  "aria-label": ariaLabel,
}: {
  value: string | undefined;
  onChange: (v: string) => void;
  children: React.ReactNode;
  className?: string;
  "aria-label"?: string;
}) {
  return (
    <div className={cn("relative inline-flex items-center", className)}>
      <select
        aria-label={ariaLabel}
        className="appearance-none border-0 bg-transparent py-1 pl-0 pr-4 text-[12px] font-medium outline-none focus:outline-none focus:ring-0"
        onChange={(e) => onChange(e.target.value)}
        value={value}
      >
        {children}
      </select>
      <ChevronDown className="pointer-events-none absolute right-0 text-current" size={11} />
    </div>
  );
}

// ─── main component ──────────────────────────────────────────────────────────

export function ChatComposer({
  draft,
  onDraftChange,
  onSend,
  onCancel,
  disabled,
  isStreaming,
  isCancelling = false,
  models,
  selectedModel,
  onModelChange,
  projects,
  activeProjectId,
  onProjectChange,
  onCreateProject,
  runtimeMode,
  onRuntimeModeChange,
  approvalMode,
  onApprovalModeChange,
  reasoningLevel = "medium",
  onReasoningLevelChange,
  showProjectSelector = true,
  compact = false,
}: ChatComposerProps) {
  const { t } = useTranslation(["chat", "common"]);
  const isBusy = Boolean(isStreaming || isCancelling);
  const [projectOpen, setProjectOpen] = useState(false);
  const [projectSearch, setProjectSearch] = useState("");
  const [addingProject, setAddingProject] = useState(false);
  const [newProjectPath, setNewProjectPath] = useState("");
  const wrapperRef = useRef<HTMLDivElement>(null);

  const activeProject = projects.find((p) => p.id === activeProjectId);
  const isProjectBound = activeProject && !isUnboundProject(activeProject.id);

  const filteredProjects = projects.filter((p) => {
    if (!projectSearch) return true;
    return (
      p.name.toLowerCase().includes(projectSearch.toLowerCase()) ||
      p.directory?.toLowerCase().includes(projectSearch.toLowerCase())
    );
  });

  const showCollapsibleProject = showProjectSelector && !compact;
  const showInlineProject = showProjectSelector && compact;

  const modelItems: DropdownItem[] = models.map((m, i) => ({
    value: m.id,
    label: m.name,
    icon: i === 0 ? <Zap size={12} /> : undefined,
  }));

  const reasoningItems: DropdownItem[] = [
    { value: "low", label: t("chat:reasoningLow") },
    { value: "medium", label: t("chat:reasoningMedium") },
    { value: "high", label: t("chat:reasoningHigh") },
    { value: "extra_high", label: t("chat:reasoningExtraHigh") },
  ];

  const sendButton = (
    <button
      className={cn(
        "flex h-8 w-8 shrink-0 items-center justify-center rounded-full outline-none transition focus:outline-none",
        isBusy
          ? "bg-danger text-white hover:bg-danger/90"
          : draft.trim().length === 0 || disabled
            ? "bg-muted/20 text-muted/60"
            : "bg-ink text-background hover:bg-ink/85",
      )}
      disabled={disabled || (!isBusy && draft.trim().length === 0)}
      onClick={() => {
        if (isBusy) {
          onCancel?.();
          return;
        }
        void onSend();
      }}
      type="button"
    >
      {isCancelling ? (
        <LoaderCircle className="animate-spin" size={14} />
      ) : isBusy ? (
        <Square size={14} />
      ) : (
        <ArrowUp size={15} strokeWidth={2.5} />
      )}
    </button>
  );

  return (
    <div className="relative" ref={wrapperRef}>
      <Panel className="overflow-visible rounded-[1.5rem] border-border/80 bg-card/95 p-0 backdrop-blur">
        {/* Compact: inline model + project row at top */}
        {showInlineProject && (
          <div className="flex flex-wrap items-center gap-2 border-b border-border/70 px-3 py-2">
            <select
              className="min-w-[10rem] flex-1 rounded-2xl border border-border bg-surface/80 px-3 py-2 text-sm outline-none transition focus:border-brand/50 md:max-w-[15rem]"
              onChange={(event) => onModelChange?.(event.target.value)}
              value={selectedModel}
            >
              {models.map((model) => (
                <option key={model.id} value={model.id}>
                  {model.name}
                </option>
              ))}
            </select>
            <select
              className="min-w-[12rem] flex-[1.2] rounded-2xl border border-border bg-surface/80 px-3 py-2 text-sm outline-none transition focus:border-brand/50 md:max-w-[22rem]"
              onChange={(event) => onProjectChange(event.target.value)}
              value={activeProjectId}
            >
              {projects.map((project) => (
                <option key={project.id} value={project.id}>
                  {isUnboundProject(project.id)
                    ? t("common:noProject")
                    : `${project.name} · ${describeProjectDirectory(project.directory)}`}
                </option>
              ))}
            </select>
          </div>
        )}

        {/* Textarea */}
        <div>
          <Textarea
            className={cn(
              "resize-none border-0 bg-transparent px-5 text-[14px] leading-[1.6] focus:border-0",
              compact ? "min-h-10 pt-3" : "min-h-12 pt-4",
            )}
            disabled={disabled}
            onChange={(event) => onDraftChange(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter" && !event.shiftKey) {
                event.preventDefault();
                void onSend();
              }
            }}
            placeholder={t("chat:askTalos")}
            value={draft}
          />
        </div>

        {/* Bottom toolbar */}
        <div className="flex flex-wrap items-center gap-1 px-3 pb-3 pt-1.5">
          <button
            className="flex h-7 w-7 shrink-0 items-center justify-center rounded-full border border-border/70 text-muted outline-none transition hover:border-brand/40 hover:text-brand focus:outline-none"
            type="button"
          >
            <Plus size={13} />
          </button>

          <InlineSelect
            aria-label="Approval mode"
            className={cn("ml-1", approvalMode === "skip" ? "text-brand" : "text-muted")}
            onChange={(v) => onApprovalModeChange(v as ApprovalMode)}
            value={approvalMode}
          >
            <option value="needs_approval">{t("chat:defaultPermissions")}</option>
            <option value="skip">{t("chat:fullAccess")}</option>
          </InlineSelect>

          <div className="min-w-4 flex-1" />

          {/* Model + Reasoning — non-compact only */}
          {!compact && (
            <>
              <ComposerDropdown
                header={t("chat:modelLabel")}
                items={modelItems}
                onChange={(v) => onModelChange?.(v)}
                triggerIcon={<Zap size={12} className="text-brand" />}
                value={selectedModel}
              />
              <ComposerDropdown
                header={t("chat:reasoningLabel")}
                items={reasoningItems}
                onChange={(v) => onReasoningLevelChange?.(v as ReasoningLevel)}
                value={reasoningLevel}
              />
            </>
          )}

          {sendButton}
        </div>

        {/* Bottom context row — project picker + runtime mode */}
        <div className="border-t border-border/50">
          <div className="flex flex-wrap items-center gap-1 px-3">
            {showCollapsibleProject && (
              <>
                <button
                  className="flex items-center gap-2 py-2.5 text-[12px] text-muted outline-none transition hover:text-ink focus:outline-none"
                  onClick={() => setProjectOpen(!projectOpen)}
                  type="button"
                >
                  <FolderOpen size={13} className="shrink-0" />
                  <span className="max-w-[220px] truncate">
                    {isProjectBound
                      ? `${activeProject.name} · ${describeProjectDirectory(activeProject.directory)}`
                      : t("chat:workInProject")}
                  </span>
                  <ChevronDown
                    className={cn("shrink-0 transition-transform", projectOpen && "rotate-180")}
                    size={13}
                  />
                </button>
                <div className="border-l border-border/40 pl-1">
                  <ComposerDropdown
                    header={t("common:runtimeMode")}
                    items={[
                      { value: "cloud", label: t("common:cloud") },
                      { value: "local", label: t("common:localDesktopOnly") },
                    ]}
                    onChange={(v) => onRuntimeModeChange(v as RuntimeMode)}
                    value={runtimeMode}
                  />
                </div>
              </>
            )}

            {/* Session page: no project selector, just runtime mode */}
            {!showCollapsibleProject && !compact && (
              <ComposerDropdown
                header={t("common:runtimeMode")}
                items={[
                  { value: "cloud", label: t("common:cloud") },
                  { value: "local", label: t("common:localDesktopOnly") },
                ]}
                onChange={(v) => onRuntimeModeChange(v as RuntimeMode)}
                value={runtimeMode}
              />
            )}
          </div>
        </div>
      </Panel>

      {/* Floating project dropdown — outside Panel so it doesn't shift card height */}
      {showCollapsibleProject && projectOpen && (
        <div className="absolute left-0 right-0 top-[calc(100%-1px)] z-50 rounded-b-[1.5rem] border border-t-0 border-border/70 bg-card/98 px-2 pb-2 pt-1 shadow-lg backdrop-blur">
          <div className="relative mb-1 mt-1">
            <Search
              className="pointer-events-none absolute left-2.5 top-1/2 -translate-y-1/2 text-muted"
              size={12}
            />
            <input
              autoFocus
              className="w-full rounded-xl border border-border/50 bg-background/60 py-1.5 pl-7 pr-3 text-[12px] outline-none placeholder:text-muted focus:border-brand/50 focus:ring-0"
              onChange={(e) => setProjectSearch(e.target.value)}
              placeholder={t("chat:searchProjects")}
              value={projectSearch}
            />
          </div>
          <div className="max-h-44 overflow-y-auto">
            {filteredProjects.map((project) => (
              <button
                className={cn(
                  "flex w-full items-center gap-2 rounded-lg px-2.5 py-2 text-left text-[12px] outline-none transition hover:bg-muted/10 focus:outline-none",
                  project.id === activeProjectId && "text-brand",
                )}
                key={project.id}
                onClick={() => {
                  onProjectChange(project.id);
                  setProjectOpen(false);
                  setProjectSearch("");
                }}
                type="button"
              >
                <FolderOpen size={12} className="shrink-0 text-muted" />
                <span className="truncate">
                  {isUnboundProject(project.id) ? t("common:noProject") : project.name}
                </span>
                {!isUnboundProject(project.id) && project.directory && (
                  <span className="ml-auto shrink-0 text-muted">
                    {describeProjectDirectory(project.directory)}
                  </span>
                )}
              </button>
            ))}
          </div>
          {addingProject ? (
            <div className="mt-1 flex gap-1.5">
              <input
                autoFocus
                className="flex-1 rounded-xl border border-border/50 bg-background/60 px-3 py-1.5 text-[12px] outline-none placeholder:text-muted focus:border-brand/50 focus:ring-0"
                onChange={(e) => setNewProjectPath(e.target.value)}
                onKeyDown={async (e) => {
                  if (e.key === "Enter" && newProjectPath.trim()) {
                    await onCreateProject?.(newProjectPath.trim());
                    setNewProjectPath("");
                    setAddingProject(false);
                    setProjectOpen(false);
                  }
                  if (e.key === "Escape") {
                    setAddingProject(false);
                    setNewProjectPath("");
                  }
                }}
                placeholder="/path/to/project"
                value={newProjectPath}
              />
              <button
                className="rounded-xl border border-border/50 bg-background/60 px-3 py-1.5 text-[12px] outline-none transition hover:border-brand/50 focus:outline-none"
                onClick={async () => {
                  if (newProjectPath.trim()) {
                    await onCreateProject?.(newProjectPath.trim());
                    setNewProjectPath("");
                    setAddingProject(false);
                    setProjectOpen(false);
                  }
                }}
                type="button"
              >
                {t("chat:addPath")}
              </button>
            </div>
          ) : (
            <button
              className="mt-0.5 flex w-full items-center gap-2 rounded-lg px-2.5 py-2 text-[12px] text-muted outline-none transition hover:bg-muted/10 hover:text-ink focus:outline-none"
              onClick={() => setAddingProject(true)}
              type="button"
            >
              <Plus size={12} />
              {t("chat:addNewProject")}
            </button>
          )}
        </div>
      )}
    </div>
  );
}
