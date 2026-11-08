import { useSessionAutomation } from "@/features/automation";
import { automationRequestError, prepareAutomationMessage } from "@/features/automation";
import { useEffect, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { useProductScope } from "@/features/computers";
import { getTalosClient } from "@/shared/api/client";
import type { AutomationMode, ModelListResponse, ModelRef, SessionInfo } from "@talos/sdk-browser";

import {
  modelInfoForRef,
  modelPickerOptions,
  modelRefForKey,
  modelRefKey,
  modelSelectionForRef,
  reasoningLevelForEffort,
  reasoningLevelForModel,
  supportedAttachmentKinds,
  useConfiguredSessionModelSelection,
  useUpdateSessionModelMutation,
  type ChatComposerProps,
  type useChatSession,
  type useProjectsView,
} from "@/features/chat";
import type { ReasoningLevel } from "@/shared/types/ui";
import type { SessionLocationState } from "./chatSessionLocation";
import { useSessionCommands } from "./useSessionCommands";
import { useSessionPermissionMode } from "./useSessionPermissionMode";
import { useSelectedSessionProject } from "./useSelectedSessionProject";

interface UseChatSessionComposerOptions {
  chat: ReturnType<typeof useChatSession>;
  locationState: SessionLocationState | null;
  modelCatalog?: ModelListResponse;
  projectsView: ReturnType<typeof useProjectsView>;
  session?: SessionInfo;
  sessionId: string;
  nodeId?: string;
  sleepingTalo: boolean;
  restoredAutomationMode?: AutomationMode;
  onRestoredAutomationModeConsumed?: () => void;
}

/** Owns session-specific model, project, permission, and command wiring for the Composer. */
export function useChatSessionComposer({
  chat,
  locationState,
  modelCatalog,
  projectsView,
  session,
  sessionId,
  nodeId,
  sleepingTalo,
  restoredAutomationMode,
  onRestoredAutomationModeConsumed,
}: UseChatSessionComposerOptions) {
  const [draftModel, setDraftModel] = useState<ModelRef | undefined>(locationState?.initialModel);
  const [draftReasoningLevel, setDraftReasoningLevel] = useState<ReasoningLevel | undefined>(
    locationState?.initialReasoningLevel,
  );
  const [draftProjectId, setDraftProjectId] = useState<string | undefined>(
    locationState?.initialProjectId,
  );
  const { mutate: updateSessionModel } = useUpdateSessionModelMutation(sessionId);
  const activeSessionModelSelection = useConfiguredSessionModelSelection({
    catalog: modelCatalog,
    selection: session?.modelSelection,
    sessionId,
    updateSelection: updateSessionModel,
  });
  const models = modelCatalog?.models;
  const currentModelRef = activeSessionModelSelection?.modelRef
    ?? draftModel
    ?? modelCatalog?.defaultModelSelection.modelRef;
  const currentModel = currentModelRef ? modelRefKey(currentModelRef) : undefined;
  const currentModelInfo = modelInfoForRef(models, currentModelRef);
  const currentReasoningLevel = activeSessionModelSelection
    ? reasoningLevelForEffort(activeSessionModelSelection.reasoningEffort)
    : draftReasoningLevel ?? (draftModel
        ? reasoningLevelForModel(models, draftModel)
        : reasoningLevelForEffort(modelCatalog?.defaultModelSelection.reasoningEffort ?? null));
  const permissionMode = useSessionPermissionMode({
    blocked: Boolean(chat.isStreaming || chat.pendingUserAction),
    initialMode: locationState?.initialPermissionMode,
    persistedMode: session?.effectiveMode,
    sessionExists: Boolean(session),
    sessionId,
  });
  const sessionCommands = useSessionCommands({
    contextCompaction: chat.contextCompaction,
    onCompactionObservationChange: chat.setCompactionObservationActive,
    sessionId,
  });
  const { boundProject, selectedProjectId } = useSelectedSessionProject({
    activeProjectId: projectsView.activeProjectId,
    draftProjectId,
    projects: projectsView.projects,
    sessionExists: Boolean(session),
    sessionProjectId: session?.projectId,
  });
  const composerModels = modelPickerOptions(models);
  const currentModelLabel = composerModels.find((model) => model.id === currentModel)?.name
    ?? currentModel
    ?? "Unknown";

  const automation = { ...useSessionAutomation(sessionId, Boolean(session)), activeTurn: chat.automationTurn };
  const scope = useProductScope();
  const owner = JSON.stringify([scope, sessionId, session?.nodeId ?? nodeId]);
  const ownerRef = useRef<string | undefined>(owner);
  ownerRef.current = owner;
  useEffect(() => { ownerRef.current = owner; return () => { ownerRef.current = undefined; }; }, [owner]);
  const initialCommands = useQuery({ queryKey: ["talos", scope, "automation-commands"],
    queryFn: () => getTalosClient().automation.commandCatalog({}),
    enabled: !session && Boolean(nodeId), staleTime: Infinity });

  const props: ChatComposerProps = {
    automation: session ? automation : undefined,
    initialAutomationMode: restoredAutomationMode,
    onRestoredAutomationModeConsumed,
    automationCommands: session ? automation.commands : initialCommands.data?.data,
    activeProjectId: selectedProjectId,
    commandResponse: sessionCommands.response,
    commandStatus: {
      model: currentModelLabel,
      project: boundProject?.name,
      reasoningLevel: currentReasoningLevel,
      sessionId,
      state: chat.isStreaming ? "running" : "ready",
    },
    contextUsage: session?.usage?.context,
    draft: chat.draft,
    initialAttachments: locationState?.initialAttachments,
    inputHistory: chat.inputHistory,
    isCancelling: chat.isCancelling,
    isStreaming: chat.isStreaming,
    messageQueue: chat.messageQueue,
    models: composerModels,
    onCancel: () => void chat.stopResponse(),
    onCommand: sessionCommands.execute,
    onDismissCommandResponse: sessionCommands.dismissResponse,
    onDraftChange: chat.setDraft,
    onModelChange: (model) => {
      if (sessionCommands.compactionStage) {
        return;
      }
      const modelRef = modelRefForKey(model);
      if (!modelRef) {
        return;
      }
      const modelLabel = composerModels.find((option) => option.id === model)?.name ?? model;
      if (!session) {
        setDraftModel(modelRef);
        const reasoningLevel = reasoningLevelForModel(models, modelRef);
        setDraftReasoningLevel(reasoningLevel);
        sessionCommands.showModelChanged(modelLabel, reasoningLevel);
        return;
      }
      const modelSelection = modelSelectionForRef(models, modelRef);
      if (modelSelection) {
        const reasoningLevel = reasoningLevelForEffort(modelSelection.reasoningEffort);
        updateSessionModel(modelSelection, {
          onError: (error) => sessionCommands.showModelChangeFailed(
            modelLabel,
            error,
            reasoningLevel,
          ),
          onSuccess: () => sessionCommands.showModelChanged(modelLabel, reasoningLevel),
        });
      }
    },
    onPermissionModeChange: permissionMode.onChange,
    onProjectChange: (projectId) => {
      if (sessionCommands.compactionStage) {
        return;
      }
      projectsView.setActiveProjectId(projectId);
      if (!session) {
        setDraftProjectId(projectId);
      }
    },
    onReasoningLevelChange: (level) => {
      if (sessionCommands.compactionStage) {
        return;
      }
      if (!session) {
        setDraftReasoningLevel(level);
        return;
      }
      const modelSelection = modelSelectionForRef(models, currentModelRef, level);
      if (modelSelection) {
        updateSessionModel(modelSelection, {
          onError: (error) => sessionCommands.showModelChangeFailed(
            currentModelLabel,
            error,
            level,
          ),
          onSuccess: () => sessionCommands.showModelChanged(currentModelLabel, level),
        });
      }
    },
    onSend: async (attachments, mode) => {
      const text = chat.draft;
      const prepared = await prepareAutomationMessage(text, mode, session?.nodeId ?? nodeId, sleepingTalo);
      if (ownerRef.current !== owner) throw new DOMException("The message could not be sent.", "AbortError");
      if (prepared.kind === "management") {
        if (!session) throw automationRequestError("chatRequired");
        await automation.command(prepared.command);
        chat.setDraft("");
        return;
      }
      const skillSelection = prepared.skillSelection;
      const fileIds = attachments.map((file) => file.fileId);
      const fileOptions = { ...(fileIds.length > 0 ? { fileIds } : {}), ...(skillSelection ? { skillSelection } : {}) };
      if (!session) {
        const selectedProject = projectsView.projects.find(
          (entry) => entry.id === selectedProjectId,
        );
        const options = {
          modelSelection: modelSelectionForRef(models, currentModelRef, currentReasoningLevel),
          nodeId,
          project: selectedProject,
          userPermissionMode: permissionMode.value,
          ...fileOptions,
        };
        return prepared.prompt !== text ? chat.sendTurn(options, prepared.prompt) : chat.sendTurn(options);
      }
      return prepared.prompt !== text
        ? chat.sendTurn(fileOptions, prepared.prompt)
        : chat.sendTurn(fileOptions);
    },
    permissionMode: permissionMode.value,
    permissionModeDisabled: permissionMode.disabled,
    projects: projectsView.projects,
    reasoningLevel: currentReasoningLevel,
    selectedModel: currentModel,
    sessionTotalTokens: session?.usage?.tokens.total_tokens,
    showProjectSelector: false,
    supportedAttachmentKinds: supportedAttachmentKinds(currentModelInfo),
  };

  return {
    boundProject,
    clearConfirmation: {
      clearing: sessionCommands.clearing,
      error: sessionCommands.clearError,
      onCancel: sessionCommands.cancelClear,
      onConfirm: sessionCommands.confirmClear,
      open: sessionCommands.clearConfirmationOpen,
    },
    props,
  };
}
