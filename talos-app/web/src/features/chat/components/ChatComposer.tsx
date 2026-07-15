import { useEffect, useMemo, useRef, useState } from "react";
import {
  ArrowLeft,
  ArrowUp,
  ChevronDown,
  FolderOpen,
  FolderPlus,
  LoaderCircle,
  Plus,
  Search,
  Shield,
  Square,
  TriangleAlert,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import type { SkillSuggestion } from "@talos/sdk-browser";

import { createFsDirectory, readFsDirectory, suggestSkills } from "../api";
import { ProjectDirectoryDialog, type ProjectDirectoryDialogMode } from "@/features/projects/components/ProjectDirectoryDialog";
import { cn } from "@/shared/lib/cn";
import { Panel } from "@/shared/ui/Panel";
import { Textarea } from "@/shared/ui/Input";
import { describeProjectDirectory, isUnboundProject } from "@/shared/types/domain";
import { ComposerDropdown, type DropdownItem } from "./ComposerDropdown";
import { ComposerModelSelect } from "./ComposerModelSelect";
import { ComposerProviderModelDropdowns } from "./ComposerProviderModelDropdowns";
import { SelectedSkillChips } from "./SelectedSkillChips";
import { SkillSuggestionsPopover } from "./SkillSuggestionsPopover";
import { activeSkillTrigger, replaceSkillTrigger } from "./skillTrigger";
import type { ProjectView, ReasoningLevel, RuntimeMode, SessionInputHistory } from "@/shared/types/ui";

interface ChatComposerProps {
  draft: string;
  onDraftChange: (value: string) => void;
  onSend: (skills: SkillSuggestion[]) => Promise<void> | void;
  initialSelectedSkills?: SkillSuggestion[];
  onCancel?: () => void;
  inputHistory?: SessionInputHistory;
  disabled?: boolean;
  isStreaming?: boolean;
  isCancelling?: boolean;
  models: Array<{ id: string; name: string; provider?: string }>;
  selectedModel?: string;
  onModelChange?: (model: string) => void;
  projects: ProjectView[];
  activeProjectId: string;
  onProjectChange: (projectId: string) => void;
  onCreateProject?: (input: string | { directory: string; name?: string }) => Promise<void>;
  runtimeMode: RuntimeMode;
  onRuntimeModeChange: (mode: RuntimeMode) => void;
  reasoningLevel?: ReasoningLevel;
  onReasoningLevelChange?: (level: ReasoningLevel) => void;
  reasoningEnabled?: boolean;
  showProjectSelector?: boolean;
  showRuntimeModeSelector?: boolean;
  compact?: boolean;
}

const EMPTY_INPUT_HISTORY_ITEMS: InputHistoryNavigationItem[] = [];
const EMPTY_SELECTED_SKILLS: SkillSuggestion[] = [];

interface InputHistoryNavigationItem {
  key: string;
  text: string;
}

type ProjectPickerMode = "list" | "add_menu" | "existing" | "create";
type ProjectMenuMode = Exclude<ProjectPickerMode, "existing" | "create">;

function projectStatusUnavailable(project: ProjectView) {
  return Boolean(project.status && project.status.state !== "available");
}

// ─── main component ──────────────────────────────────────────────────────────

export function ChatComposer({
  draft,
  onDraftChange,
  onSend,
  initialSelectedSkills = EMPTY_SELECTED_SKILLS,
  onCancel,
  inputHistory,
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
  reasoningLevel = "medium",
  onReasoningLevelChange,
  reasoningEnabled = true,
  showProjectSelector = true,
  showRuntimeModeSelector = true,
  compact = false,
}: ChatComposerProps) {
  const { t } = useTranslation(["chat", "common", "settings"]);
  const isBusy = Boolean(isStreaming || isCancelling);
  const [projectOpen, setProjectOpen] = useState(false);
  const [projectSearch, setProjectSearch] = useState("");
  const [projectMode, setProjectMode] = useState<ProjectMenuMode>("list");
  const [skillSuggestions, setSkillSuggestions] = useState<SkillSuggestion[]>([]);
  const [selectedSkills, setSelectedSkills] = useState<SkillSuggestion[]>(initialSelectedSkills);
  const [skillSuggestionIndex, setSkillSuggestionIndex] = useState(0);
  const [skillCursorIndex, setSkillCursorIndex] = useState(() => draft.length);
  const [directoryDialogMode, setDirectoryDialogMode] = useState<ProjectDirectoryDialogMode | null>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const projectTriggerRef = useRef<HTMLButtonElement>(null);
  const projectPickerRef = useRef<HTMLDivElement>(null);
  const historyIndexRef = useRef<number | null>(null);
  const historyDraftRef = useRef("");
  const activeInputHistoryItems = useMemo(
    () => buildInputHistoryNavigationItems(inputHistory),
    [inputHistory],
  );
  const activeInputHistoryKey = useMemo(
    () => activeInputHistoryItems.map((item) => item.key).join("|"),
    [activeInputHistoryItems],
  );

  const closeProjectPicker = () => {
    setProjectOpen(false);
    setProjectSearch("");
    setProjectMode("list");
  };

  const resetInputHistoryNavigation = () => {
    historyIndexRef.current = null;
    historyDraftRef.current = "";
  };

  useEffect(() => {
    if (!projectOpen) {
      return;
    }

    function onPointerDown(event: PointerEvent) {
      const target = event.target as Node | null;
      if (!target) {
        return;
      }
      if (projectTriggerRef.current?.contains(target) || projectPickerRef.current?.contains(target)) {
        return;
      }
      closeProjectPicker();
    }

    function onKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") {
        closeProjectPicker();
      }
    }

    document.addEventListener("pointerdown", onPointerDown);
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [projectOpen]);

  useEffect(() => {
    historyIndexRef.current = null;
    historyDraftRef.current = "";
  }, [activeInputHistoryKey]);

  useEffect(() => {
    const trigger = activeSkillTrigger(draft, Math.min(skillCursorIndex, draft.length));
    if (!trigger) {
      setSkillSuggestions([]);
      setSkillSuggestionIndex(0);
      setSelectedSkills((skills) => skills.filter((skill) => draft.includes(`$${skill.name}`)));
      return;
    }

    let cancelled = false;
    const timer = window.setTimeout(() => {
      void suggestSkills(trigger.query)
        .then((skills) => {
          if (!cancelled) {
            setSkillSuggestions(skills);
            setSkillSuggestionIndex(0);
          }
        })
        .catch(() => {
          if (!cancelled) {
            setSkillSuggestions([]);
          }
        });
    }, 120);

    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [draft, skillCursorIndex]);

  useEffect(() => {
    setSelectedSkills(initialSelectedSkills);
  }, [initialSelectedSkills]);

  const activeProject = projects.find((p) => p.id === activeProjectId);
  const isProjectBound = activeProject && !isUnboundProject(activeProject.id);

  const filteredProjects = projects.filter((p) => {
    if (!projectSearch) return true;
    return (
      p.name.toLowerCase().includes(projectSearch.toLowerCase()) ||
      p.directory?.toLowerCase().includes(projectSearch.toLowerCase())
    );
  });

  function openDirectoryDialog(mode: ProjectDirectoryDialogMode) {
    closeProjectPicker();
    setDirectoryDialogMode(mode);
  }

  const showCollapsibleProject = showProjectSelector && !compact;
  const showInlineProject = showProjectSelector && compact;
  const effectiveRuntimeMode: RuntimeMode = runtimeMode === "local" ? "local" : "local";
  const sendButtonDisabled = isBusy ? false : Boolean(disabled || draft.trim().length === 0);
  const sendButtonLabel = isBusy ? t("common:stopResponse") : t("chat:send");

  const moveCursorToPosition = (textarea: HTMLTextAreaElement, cursorIndex: number) => {
    const applySelection = () => {
      textarea.setSelectionRange(cursorIndex, cursorIndex);
      setSkillCursorIndex(cursorIndex);
    };
    if (typeof window.requestAnimationFrame === "function") {
      window.requestAnimationFrame(applySelection);
      return;
    }
    applySelection();
  };

  const setDraftFromHistory = (textarea: HTMLTextAreaElement, value: string) => {
    onDraftChange(value);
    moveCursorToPosition(textarea, value.length);
  };

  const canUseInputHistory = (event: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (
      disabled ||
      event.altKey ||
      event.ctrlKey ||
      event.metaKey ||
      event.nativeEvent.isComposing ||
      activeInputHistoryItems.length === 0
    ) {
      return false;
    }

    if (historyIndexRef.current !== null) {
      return true;
    }

    const textarea = event.currentTarget;
    const cursorAtEnd =
      textarea.selectionStart === textarea.value.length &&
      textarea.selectionEnd === textarea.value.length;
    return textarea.value.length === 0 || cursorAtEnd;
  };

  const handleInputHistoryKeyDown = (event: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (!canUseInputHistory(event)) {
      return false;
    }

    if (event.key === "ArrowUp") {
      event.preventDefault();
      const nextIndex =
        historyIndexRef.current === null
          ? activeInputHistoryItems.length - 1
          : Math.max(0, historyIndexRef.current - 1);
      if (historyIndexRef.current === null) {
        historyDraftRef.current = draft;
      }
      historyIndexRef.current = nextIndex;
      setDraftFromHistory(event.currentTarget, activeInputHistoryItems[nextIndex]?.text ?? "");
      return true;
    }

    if (event.key === "ArrowDown" && historyIndexRef.current !== null) {
      event.preventDefault();
      const nextIndex = historyIndexRef.current + 1;
      if (nextIndex >= activeInputHistoryItems.length) {
        const restoredDraft = historyDraftRef.current;
        resetInputHistoryNavigation();
        setDraftFromHistory(event.currentTarget, restoredDraft);
        return true;
      }

      historyIndexRef.current = nextIndex;
      setDraftFromHistory(event.currentTarget, activeInputHistoryItems[nextIndex]?.text ?? "");
      return true;
    }

    return false;
  };

  const handleSend = () => {
    resetInputHistoryNavigation();
    const skills = selectedSkills;
    setSelectedSkills([]);
    setSkillSuggestions([]);
    return onSend(skills);
  };

  const selectSkill = (skill: SkillSuggestion, textarea?: HTMLTextAreaElement | null) => {
    const cursorIndex =
      textarea && document.activeElement === textarea ? textarea.selectionStart : skillCursorIndex;
    const trigger = activeSkillTrigger(draft, cursorIndex);
    if (!trigger) {
      return;
    }
    const next = replaceSkillTrigger(draft, trigger, skill.name);
    setSelectedSkills((skills) => {
      if (skills.some((item) => item.path === skill.path)) {
        return skills;
      }
      return [...skills, skill];
    });
    setSkillSuggestions([]);
    onDraftChange(next.value);
    if (textarea) {
      moveCursorToPosition(textarea, next.cursorIndex);
    }
  };

  const reasoningItems: DropdownItem[] = reasoningEnabled
    ? [
        { value: "light", label: t("chat:reasoningLight") },
        { value: "medium", label: t("chat:reasoningMedium") },
        { value: "high", label: t("chat:reasoningHigh") },
        { value: "extra_high", label: t("chat:reasoningExtraHigh") },
      ]
    : [{ value: "none", label: t("chat:reasoningNone") }];

  const sendButton = (
    <button
      aria-label={sendButtonLabel}
      className={cn(
        "flex h-8 w-8 shrink-0 items-center justify-center rounded-full outline-none transition focus:outline-none",
        isBusy
          ? "bg-danger text-white hover:bg-danger/90"
          : draft.trim().length === 0 || disabled
            ? "bg-muted/20 text-muted/60"
            : "bg-ink text-background hover:bg-ink/85",
      )}
      data-smoke="chat-composer-send"
      disabled={sendButtonDisabled}
      onClick={() => {
        if (isBusy) {
          onCancel?.();
          return;
        }
        void handleSend();
      }}
      title={sendButtonLabel}
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
    <div className="relative">
      <Panel className="overflow-visible rounded-[1.5rem] border-border/80 bg-card p-0 backdrop-blur-none">
        {showInlineProject && (
          <div className="flex flex-wrap items-center gap-2 border-b border-border/70 px-3 py-2">
            <ComposerModelSelect
              modelLabel={t("chat:modelLabel")}
              models={models}
              onChange={onModelChange}
              providerLabel={t("settings:modelsPanel.activeProvider")}
              selectedModel={selectedModel}
            />
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

        <div>
          <SelectedSkillChips
            draft={draft}
            onDraftChange={onDraftChange}
            onSkillsChange={setSelectedSkills}
            skills={selectedSkills}
          />
          <Textarea
            className={cn(
              "resize-none border-0 bg-transparent px-5 text-[14px] leading-[1.6] focus:border-0",
              compact ? "min-h-10 pt-3" : "min-h-12 pt-4",
            )}
            data-smoke="chat-composer-input"
            disabled={disabled}
            onChange={(event) => {
              resetInputHistoryNavigation();
              setSkillCursorIndex(event.currentTarget.selectionStart);
              onDraftChange(event.target.value);
            }}
            onClick={(event) => setSkillCursorIndex(event.currentTarget.selectionStart)}
            onKeyDown={(event) => {
              if (skillSuggestions.length > 0) {
                if (event.key === "ArrowDown") {
                  event.preventDefault();
                  setSkillSuggestionIndex((index) => Math.min(skillSuggestions.length - 1, index + 1));
                  return;
                }
                if (event.key === "ArrowUp") {
                  event.preventDefault();
                  setSkillSuggestionIndex((index) => Math.max(0, index - 1));
                  return;
                }
                if (event.key === "Enter" || event.key === "Tab") {
                  event.preventDefault();
                  selectSkill(skillSuggestions[skillSuggestionIndex] ?? skillSuggestions[0], event.currentTarget);
                  return;
                }
                if (event.key === "Escape") {
                  event.preventDefault();
                  setSkillSuggestions([]);
                  return;
                }
              }
              if (event.key === "Enter" && !event.shiftKey) {
                event.preventDefault();
                if (isBusy) {
                  return;
                }
                void handleSend();
                return;
              }
              handleInputHistoryKeyDown(event);
            }}
            onKeyUp={(event) => setSkillCursorIndex(event.currentTarget.selectionStart)}
            placeholder={t("chat:askTalos")}
            ref={textareaRef}
            value={draft}
          />
          <SkillSuggestionsPopover
            onHover={setSkillSuggestionIndex}
            onSelect={(skill) => selectSkill(skill, textareaRef.current)}
            selectedIndex={skillSuggestionIndex}
            suggestions={skillSuggestions}
          />
        </div>

        <div className="flex flex-wrap items-center gap-1 px-3 pb-3 pt-1.5">
          <button
            className="flex h-7 w-7 shrink-0 items-center justify-center rounded-full border border-border/70 text-muted outline-none transition hover:border-brand/40 hover:text-brand focus:outline-none"
            type="button"
          >
            <Plus size={13} />
          </button>

          <ComposerDropdown
            header={t("chat:agentAccessMode")}
            items={[
              {
                value: "full_access",
                label: t("chat:fullAccess"),
                description: t("chat:fullAccessDescription"),
                icon: <Shield size={13} />,
              },
            ]}
            onChange={() => undefined}
            value="full_access"
          />

          <div className="min-w-4 flex-1" />

          {!compact && (
            <>
              <ComposerProviderModelDropdowns
                modelLabel={t("chat:modelLabel")}
                models={models}
                onChange={onModelChange}
                providerLabel={t("settings:modelsPanel.activeProvider")}
                selectedModel={selectedModel}
              />
              <ComposerDropdown
                disabled={!reasoningEnabled}
                header={t("chat:reasoningLabel")}
                items={reasoningItems}
                onChange={(v) => onReasoningLevelChange?.(v as ReasoningLevel)}
                value={reasoningLevel}
              />
            </>
          )}

          {sendButton}
        </div>

        <div className="border-t border-border/50">
          <div className="flex flex-wrap items-center gap-1 px-3">
            {showCollapsibleProject && (
              <>
                <button
                  className="flex items-center gap-2 py-2.5 text-[12px] text-muted outline-none transition hover:text-ink focus:outline-none"
                  onClick={() => setProjectOpen(!projectOpen)}
                  ref={projectTriggerRef}
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
                {showRuntimeModeSelector && (
                  <div className="border-l border-border/40 pl-1">
                    <ComposerDropdown
                      header={t("common:runtimeMode")}
                      items={[
                        { value: "local", label: t("common:local") },
                        {
                          value: "cloud",
                          label: t("common:cloud"),
                          description: t("common:comingSoon"),
                          disabled: true,
                        },
                      ]}
                      onChange={(v) => onRuntimeModeChange(v as RuntimeMode)}
                      value={effectiveRuntimeMode}
                    />
                  </div>
                )}
              </>
            )}

            {showRuntimeModeSelector && !showCollapsibleProject && !compact && (
              <ComposerDropdown
                header={t("common:runtimeMode")}
                items={[
                  { value: "local", label: t("common:local") },
                  {
                    value: "cloud",
                    label: t("common:cloud"),
                    description: t("common:comingSoon"),
                    disabled: true,
                  },
                ]}
                onChange={(v) => onRuntimeModeChange(v as RuntimeMode)}
                value={effectiveRuntimeMode}
              />
            )}
          </div>
        </div>
      </Panel>

      {showCollapsibleProject && projectOpen && (
        <div
          className="absolute left-0 right-0 top-[calc(100%-1px)] z-50 rounded-b-[1.5rem] border border-t-0 border-border/70 bg-card px-2 pb-2 pt-1 shadow-lg"
          ref={projectPickerRef}
        >
          {projectMode === "list" ? (
            <>
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
                {filteredProjects.map((project) => {
                  const unavailable = projectStatusUnavailable(project);
                  return (
                    <button
                      className={cn(
                        "flex w-full items-center gap-2 rounded-lg px-2.5 py-2 text-left text-[12px] outline-none transition hover:bg-muted/10 focus:outline-none",
                        project.id === activeProjectId && "text-brand",
                        unavailable && "text-muted",
                      )}
                      key={project.id}
                      onClick={() => {
                        if (unavailable) {
                          return;
                        }
                        onProjectChange(project.id);
                        closeProjectPicker();
                      }}
                      title={unavailable ? t("chat:projectDirectoryUnavailable") : undefined}
                      type="button"
                    >
                      <FolderOpen size={12} className="shrink-0 text-muted" />
                      <span className="truncate">
                        {isUnboundProject(project.id) ? t("common:noProject") : project.name}
                      </span>
                      {unavailable ? (
                        <TriangleAlert className="ml-auto shrink-0 text-danger" size={12} />
                      ) : !isUnboundProject(project.id) && project.directory ? (
                        <span className="ml-auto shrink-0 text-muted">
                          {describeProjectDirectory(project.directory)}
                        </span>
                      ) : null}
                    </button>
                  );
                })}
              </div>
              <button
                className="mt-0.5 flex w-full items-center gap-2 rounded-lg px-2.5 py-2 text-[12px] text-muted outline-none transition hover:bg-muted/10 hover:text-ink focus:outline-none"
                onClick={() => setProjectMode("add_menu")}
                type="button"
              >
                <FolderPlus size={12} />
                {t("chat:addNewProject")}
                <ChevronDown className="-rotate-90 ml-auto" size={12} />
              </button>
            </>
          ) : projectMode === "add_menu" ? (
            <div className="py-1">
              <button
                className="mb-1 flex w-full items-center gap-2 rounded-lg px-2.5 py-2 text-left text-[12px] text-muted outline-none transition hover:bg-muted/10 hover:text-ink focus:outline-none"
                onClick={() => setProjectMode("list")}
                type="button"
              >
                <ArrowLeft size={12} />
                {t("chat:backToProjects")}
              </button>
              <button
                className="flex w-full items-center gap-2 rounded-lg px-2.5 py-2 text-left text-[13px] text-ink outline-none transition hover:bg-muted/10 focus:outline-none"
                onClick={() => openDirectoryDialog("create")}
                type="button"
              >
                <Plus size={14} />
                {t("chat:startFromScratch")}
              </button>
              <button
                className="flex w-full items-center gap-2 rounded-lg px-2.5 py-2 text-left text-[13px] text-ink outline-none transition hover:bg-muted/10 focus:outline-none"
                onClick={() => openDirectoryDialog("existing")}
                type="button"
              >
                <FolderOpen size={14} />
                {t("chat:useExistingFolder")}
              </button>
            </div>
          ) : null}
        </div>
      )}
      {onCreateProject ? (
        <ProjectDirectoryDialog
          createDirectory={createFsDirectory}
          mode={directoryDialogMode}
          onClose={() => setDirectoryDialogMode(null)}
          onCreateProject={onCreateProject}
          readDirectory={readFsDirectory}
        />
      ) : null}
    </div>
  );
}

function buildInputHistoryNavigationItems(inputHistory: SessionInputHistory | undefined) {
  if (!inputHistory) {
    return EMPTY_INPUT_HISTORY_ITEMS;
  }

  const items = inputHistory.committed.map((item) => ({
    key: `${item.messageId}:${item.itemIndex}`,
    text: item.text,
  }));
  const pendingInput = inputHistory.pendingInput;
  const pendingCommitted =
    Boolean(pendingInput?.serverMessageId) &&
    inputHistory.committed.some((item) => item.messageId === pendingInput?.serverMessageId);

  if (isNavigablePendingInput(inputHistory) && pendingInput && !pendingCommitted) {
    items.push({
      key: [
        "pending",
        pendingInput.clientMessageId,
        pendingInput.serverMessageId ?? "",
        pendingInput.phase,
      ].join(":"),
      text: pendingInput.text,
    });
  }

  return items;
}

function isNavigablePendingInput(inputHistory: SessionInputHistory) {
  const pendingInput = inputHistory.pendingInput;
  return pendingInput?.phase === "local" || pendingInput?.phase === "accepted";
}
