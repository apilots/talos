import { useState } from "react";
import type { ModelListResponse, ModelRef, SessionInfo } from "@talos/sdk-browser";

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
import { useAppStore } from "@/shared/store/app-store";
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
}

/** Owns session-specific model, project, permission, and command wiring for the Composer. */
export function useChatSessionComposer({
  chat,
  locationState,
  modelCatalog,
  projectsView,
  session,
  sessionId,
}: UseChatSessionComposerOptions) {
  const [draftModel, setDraftModel] = useState<ModelRef | undefined>(locationState?.initialModel);
  const [draftReasoningLevel, setDraftReasoningLevel] = useState<ReasoningLevel | undefined>(
    locationState?.initialReasoningLevel,
  );
  const [draftProjectId, setDraftProjectId] = useState<string | undefined>(
    locationState?.initialProjectId,
  );
  const { mutate: updateSessionModel } = useUpdateSessionModelMutation(sessionId);
  const connection = useAppStore((state) => state.connection);
  const setConnection = useAppStore((state) => state.setConnection);
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

  const props: ChatComposerProps = {
    activeProjectId: selectedProjectId,
    commandResponse: sessionCommands.response,
    commandStatus: {
      model: currentModelLabel,
      project: boundProject?.name,
      reasoningLevel: currentReasoningLevel,
      runtimeMode: connection.runtimeMode,
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
    onRuntimeModeChange: (mode) => {
      if (!sessionCommands.compactionStage && mode === "local") {
        setConnection({ runtimeMode: mode });
      }
    },
    onSend: (attachments) => {
      const fileIds = attachments.map((file) => file.fileId);
      const fileOptions = fileIds.length > 0 ? { fileIds } : {};
      if (!session) {
        const selectedProject = projectsView.projects.find(
          (entry) => entry.id === selectedProjectId,
        );
        return chat.sendTurn({
          modelSelection: modelSelectionForRef(models, currentModelRef, currentReasoningLevel),
          project: selectedProject,
          userPermissionMode: permissionMode.value,
          ...fileOptions,
        });
      }
      return chat.sendTurn(fileOptions);
    },
    permissionMode: permissionMode.value,
    permissionModeDisabled: permissionMode.disabled,
    projects: projectsView.projects,
    reasoningEnabled: currentModelInfo?.reasoning ?? false,
    reasoningLevel: currentReasoningLevel,
    runtimeMode: connection.runtimeMode,
    selectedModel: currentModel,
    sessionTotalTokens: session?.usage?.tokens.total_tokens,
    showProjectSelector: false,
    showRuntimeModeSelector: false,
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
