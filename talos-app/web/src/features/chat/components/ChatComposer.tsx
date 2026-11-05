import { apiErrorMessage, userVisibleError } from "@/shared/i18n/apiErrorMessage";
import { AutomationEditDialog } from "@/features/automation/AutomationEditDialog";
import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { cn } from "@/shared/lib/cn";
import { Panel } from "@/shared/ui/Panel";
import { reasoningLevelForEffort } from "../model-selection";
import { ComposerAttachmentTray } from "./ComposerAttachmentTray";
import { ComposerAttachments } from "./ComposerAttachments";
import { ComposerCompactContext } from "./ComposerCompactContext";
import { ComposerAutomationMode } from "./ComposerAutomationMode";
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
  initialAutomationMode,
  onRestoredAutomationModeConsumed,
  automationCommands,
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
  reasoningLevel: configuredReasoningLevel,
  onReasoningLevelChange,
  showProjectSelector = true,
  compact = false,
  initialAttachments = EMPTY_ATTACHMENTS,
  supportedAttachmentKinds = EMPTY_ATTACHMENT_KINDS,
  messageQueue,
  automation,
}: Composer.ChatComposerProps) {
  const { t } = useTranslation(["chat", "common", "settings"]);
  const selectionCapabilities = models.find((model) => model.id === selectedModel)?.selectionCapabilities;
  const reasoningLevel = configuredReasoningLevel ?? reasoningLevelForEffort(selectionCapabilities?.default_reasoning_effort ?? null);
  const commandRunning = commandResponse?.kind === "compact" && commandResponse.state === "running";
  const isBusy = Boolean(isStreaming || isCancelling || commandRunning);
  const [pendingMode, setPendingMode] = useState(initialAutomationMode);
  useEffect(() => {
    if (!initialAutomationMode) return;
    setPendingMode(initialAutomationMode);
    onRestoredAutomationModeConsumed?.();
  }, [initialAutomationMode, onRestoredAutomationModeConsumed]);
  const [sendError, setSendError] = useState<ReturnType<typeof userVisibleError>>();
  const [submitting, setSubmitting] = useState(false);
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
    () => [...availableComposerCommands({
      hasModels: !isBusy && models.length > 0 && Boolean(onModelChange),
      hasSessionCommands: Boolean(onCommand) && !isBusy,
    }), ...((automationCommands ?? automation?.commands)?.map((entry) => entry.mode) ?? [])],
    [isBusy, models.length, onCommand, onModelChange, automation?.commands, automationCommands],
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
    onAutomationMode: setPendingMode,
    onDraftChange,
    onResetHistory: inputHistoryNavigation.reset,
    session: composerSuggestions,
  });

  const handleSend = async () => {
    if (submitting || !attachmentDraft.canSubmit(draft)) return;
    inputHistoryNavigation.reset();
    composerSuggestions.cancelCurrentTrigger();
    setSubmitting(true); setSendError(undefined);
    const selected = pendingMode;
    try {
      await attachmentDraft.submit(draft, (attachments) => onSend(attachments, selected));
      setPendingMode((current) => current === selected ? undefined : current);
    } catch (failure) {
      setSendError(userVisibleError(failure, "The message could not be sent."));
    } finally { setSubmitting(false); }
  };

  return (
    <div className="relative">
      {automation?.editing ? <AutomationEditDialog fallbackFocusRef={editorRef} task={automation.editing} pending={automation.pending} error={automation.error} onClose={() => automation.setEditing(undefined)} onSave={automation.execute} /> : null}
      {!composerSuggestions.open && !modelPickerOpen ? (
        <ComposerCommandResponse
          contextUsage={contextUsage ?? undefined}
          onDismiss={onDismissCommandResponse}
          response={commandResponse}
          sessionTotalTokens={sessionTotalTokens}
          status={commandStatus}
        />
      ) : null}
      <Panel className="overflow-visible rounded-[22px] border-border bg-card p-0 shadow-none">
        {messageQueue || automation ? <WaitingMessages controller={messageQueue} automation={automation} /> : null}
        {sendError ? <p role="alert" className="px-5 pt-3 text-xs text-danger">{apiErrorMessage(sendError)}</p> : null}
        {showProjectSelector && compact ? <ComposerCompactContext {...{ disabled, isBusy, modelTriggerRef, models, onModelChange, setModelPickerOpen, onReasoningLevelChange, reasoningLevel, selectedModel, onProjectChange, activeProjectId, projects }} /> : null}

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
              if (event.key === "Enter" && (isComposing || event.nativeEvent.isComposing)) return;
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
            onAutomationMode={(automationCommands ?? automation?.commands)?.length ? (mode) => { setPendingMode(mode); editorRef.current?.focus(); } : undefined}
            availableFiles={attachmentDraft.availableFiles}
            disabled={Boolean(disabled || submitting)}
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

          {pendingMode ? <ComposerAutomationMode mode={pendingMode} onClear={() => setPendingMode(undefined)} /> : null}
          <ComposerRunControls
            canSubmit={!submitting && attachmentDraft.canSubmit(draft)}
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
