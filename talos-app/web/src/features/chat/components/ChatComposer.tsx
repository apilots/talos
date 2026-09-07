import { useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { cn } from "@/shared/lib/cn";
import { Panel } from "@/shared/ui/Panel";
import { describeProjectDirectory, isUnboundProject } from "@/shared/types/domain";
import type { DropdownItem } from "./ComposerDropdown";
import { ComposerAttachmentTray } from "./ComposerAttachmentTray";
import { ComposerAttachments } from "./ComposerAttachments";
import { ComposerModelSelect } from "./ComposerModelSelect";
import { ComposerRunControls } from "./ComposerRunControls";
import { ComposerPermissionModeSelect } from "./ComposerPermissionModeSelect";
import { ComposerProjectContext } from "./ComposerProjectContext";
import { ComposerSuggestionMenu } from "./composer-input/ComposerSuggestionMenu";
import { availableComposerCommands } from "./composer-input/composer-suggestions";
import { useComposerSuggestions } from "./composer-input/useComposerSuggestions";
import { useComposerSuggestionActions } from "./composer-input/useComposerSuggestionActions";
import { useComposerInputHistory } from "./composer-input/useComposerInputHistory";
import { EMPTY_ATTACHMENT_KINDS, EMPTY_ATTACHMENTS } from "./ChatComposer.defaults";
import type * as Composer from "./ChatComposer.types";
import { useChatComposerAttachmentDraft } from "./useChatComposerAttachmentDraft";
import { WaitingMessages } from "./WaitingMessages";
import { ComposerCommandResponse } from "./command-response";

// ─── main component ──────────────────────────────────────────────────────────

export function ChatComposer({
  draft,
  onDraftChange,
  onSend,
  onCommand,
  commandResponse,
  commandStatus,
  onDismissCommandResponse,
  onCancel,
  inputHistory,
  disabled,
  permissionMode = "fullAccess",
  permissionModeDisabled = false,
  onPermissionModeChange,
  isStreaming,
  isCancelling = false,
  contextUsage,
  sessionTotalTokens,
  models,
  selectedModel,
  onModelChange,
  projects,
  activeProjectId,
  onProjectChange,
  onCreateProject,
  reasoningLevel = "medium",
  onReasoningLevelChange,
  reasoningEnabled = true,
  showProjectSelector = true,
  compact = false,
  initialAttachments = EMPTY_ATTACHMENTS,
  supportedAttachmentKinds = EMPTY_ATTACHMENT_KINDS,
  messageQueue,
}: Composer.ChatComposerProps) {
  const { t } = useTranslation(["chat", "common", "settings"]);
  const commandRunning = commandResponse?.kind === "compact" && commandResponse.state === "running";
  const isBusy = Boolean(isStreaming || isCancelling || commandRunning);
  const [cursorIndex, setCursorIndex] = useState(() => draft.length);
  const [isComposing, setIsComposing] = useState(false);
  const [modelPickerOpen, setModelPickerOpen] = useState(false);
  const editorRef = useRef<HTMLTextAreaElement>(null);
  const modelTriggerRef = useRef<HTMLButtonElement>(null);
  const attachmentDraft = useChatComposerAttachmentDraft({
    activeProjectId,
    supportedKinds: supportedAttachmentKinds,
    initialAttachments,
  });
  const commandIds = useMemo(
    () => availableComposerCommands({
      hasModels: !isBusy && models.length > 0 && Boolean(onModelChange),
      hasSessionCommands: Boolean(onCommand) && !isBusy,
    }),
    [isBusy, models.length, onCommand, onModelChange],
  );
  const composerSuggestions = useComposerSuggestions({
    commands: commandIds,
    cursorIndex,
    enabled: !disabled,
    isComposing,
    value: draft,
  });

  const moveCursorToPosition = (nextCursorIndex: number) => {
    const applySelection = () => {
      editorRef.current?.focus();
      editorRef.current?.setSelectionRange(nextCursorIndex, nextCursorIndex);
      setCursorIndex(nextCursorIndex);
    };
    if (typeof window.requestAnimationFrame === "function") {
      window.requestAnimationFrame(applySelection);
      return;
    }
    applySelection();
  };

  const inputHistoryNavigation = useComposerInputHistory({
    disabled,
    inputHistory,
    moveCursorToPosition,
    onDraftChange,
    value: draft,
  });
  const suggestionActions = useComposerSuggestionActions({
    draft,
    modelTriggerRef,
    moveCursorToPosition,
    onCommand,
    onDraftChange,
    onResetHistory: inputHistoryNavigation.reset,
    session: composerSuggestions,
  });

  const handleSend = async () => {
    if (!attachmentDraft.canSubmit(draft)) {
      return;
    }
    inputHistoryNavigation.reset();
    composerSuggestions.cancelCurrentTrigger();
    await attachmentDraft.submit(draft, onSend);
  };

  const reasoningItems: DropdownItem[] = reasoningEnabled
    ? [
        { value: "light", label: t("chat:reasoningLight") },
        { value: "medium", label: t("chat:reasoningMedium") },
        { value: "high", label: t("chat:reasoningHigh") },
        { value: "extra_high", label: t("chat:reasoningExtraHigh") },
      ]
    : [{ value: "none", label: t("chat:reasoningNone") }];

  return (
    <div className="relative">
      {!composerSuggestions.open && !modelPickerOpen ? (
        <ComposerCommandResponse
          contextUsage={contextUsage ?? undefined}
          onDismiss={onDismissCommandResponse}
          response={commandResponse}
          sessionTotalTokens={sessionTotalTokens}
          status={commandStatus}
        />
      ) : null}
      <Panel className="overflow-visible rounded-[1.25rem] border-border bg-card p-0 shadow-panel">
        {messageQueue ? <WaitingMessages controller={messageQueue} /> : null}
        {showProjectSelector && compact ? (
          <div className="flex flex-wrap items-center gap-2 border-b border-border/70 px-3 py-2">
            <ComposerModelSelect
              disabled={Boolean(disabled || isBusy)}
              modelLabel={t("chat:modelLabel")}
              models={models}
              onChange={onModelChange}
              providerLabel={t("chat:providerLabel")}
              selectedModel={selectedModel}
            />
            <select
              className="form-control-focus min-w-[12rem] flex-[1.2] rounded-2xl border border-border bg-surface/80 px-3 py-2 text-sm transition md:max-w-[22rem]"
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
        ) : null}

        <ComposerAttachmentTray
          disabled={Boolean(disabled)}
          error={
            attachmentDraft.hasInvalidAttachments
              ? t("chat:invalidAttachments")
              : attachmentDraft.error
          }
          items={attachmentDraft.items}
          onRemove={attachmentDraft.remove}
        />
        <div>
          <textarea
            aria-label={t("chat:askTalos")}
            className={cn(
              "[field-sizing:content] max-h-40 w-full resize-none overflow-y-auto border-0 bg-transparent px-5 text-[14px] leading-[1.6] text-ink outline-none placeholder:text-muted",
              compact ? "min-h-10 py-3" : "min-h-12 py-4",
            )}
            data-smoke="chat-composer-input"
            disabled={disabled}
            onChange={(event) => {
              const nextCursorIndex = event.currentTarget.selectionStart;
              inputHistoryNavigation.reset();
              composerSuggestions.updateTriggerSession(event.currentTarget.value, nextCursorIndex);
              setCursorIndex(nextCursorIndex);
              onDraftChange(event.currentTarget.value);
            }}
            onCompositionEnd={() => setIsComposing(false)}
            onCompositionStart={() => setIsComposing(true)}
            onClick={(event) => setCursorIndex(event.currentTarget.selectionStart)}
            onKeyDown={(event) => {
              if (suggestionActions.handleKeyDown(event)) return;
              if (event.key === "Enter" && !event.shiftKey) {
                event.preventDefault();
                if (!commandRunning) void handleSend();
                return;
              }
              inputHistoryNavigation.handleKeyDown(event);
            }}
            onKeyUp={(event) => setCursorIndex(event.currentTarget.selectionStart)}
            onPaste={attachmentDraft.handlePaste}
            placeholder={t("chat:askTalos")}
            ref={editorRef}
            rows={1}
            value={draft}
          />
          <ComposerSuggestionMenu
            contextUsage={contextUsage ?? undefined}
            error={composerSuggestions.error}
            loading={composerSuggestions.loading}
            onHover={composerSuggestions.setSelectedIndex}
            onSelect={suggestionActions.selectSuggestion}
            open={composerSuggestions.open}
            selectedIndex={composerSuggestions.selectedIndex}
            suggestions={composerSuggestions.suggestions}
          />
        </div>

        <div className="flex flex-wrap items-center gap-1 px-3 pb-3 pt-1.5">
          <ComposerAttachments
            availableFiles={attachmentDraft.availableFiles}
            disabled={Boolean(disabled)}
            hasMoreAvailable={attachmentDraft.hasMoreAvailable}
            isLoading={attachmentDraft.isLoading}
            isLoadingMore={attachmentDraft.isLoadingMore}
            onLoadAvailable={attachmentDraft.loadAvailable}
            onLoadMoreAvailable={attachmentDraft.loadMoreAvailable}
            onSelect={attachmentDraft.select}
            onUpload={attachmentDraft.upload}
            supportedKinds={supportedAttachmentKinds}
          />

          <ComposerPermissionModeSelect
            disabled={permissionModeDisabled}
            onChange={onPermissionModeChange}
            value={permissionMode}
          />

          <div className="min-w-4 flex-1" />

          <ComposerRunControls
            canSubmit={attachmentDraft.canSubmit(draft)}
            compact={compact}
            contextUsage={contextUsage}
            disabled={disabled}
            isBusy={isBusy}
            isCancelling={isCancelling}
            modelTriggerRef={modelTriggerRef}
            models={models}
            onCancel={onCancel}
            onModelChange={onModelChange}
            onModelPickerOpenChange={setModelPickerOpen}
            onReasoningLevelChange={onReasoningLevelChange}
            onSend={handleSend}
            reasoningItems={reasoningItems}
            reasoningLevel={reasoningLevel}
            selectedModel={selectedModel}
            sessionTotalTokens={sessionTotalTokens}
          />
        </div>

        <ComposerProjectContext
          activeProjectId={activeProjectId}
          compact={compact}
          onCreateProject={onCreateProject}
          onProjectChange={onProjectChange}
          projects={projects}
          showProjectSelector={showProjectSelector}
        />
      </Panel>
    </div>
  );
}
