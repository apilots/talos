import { useMemo, useState } from "react";
import { useLocation, useNavigate, useParams } from "react-router-dom";

import {
  ChatComposer,
  supportedAttachmentKinds,
  modelInfoForRef,
  modelPickerOptions,
  modelRefForKey,
  modelRefKey,
  modelSelectionForRef,
  reasoningLevelForEffort,
  reasoningLevelForModel,
  useChatSession,
  useConfiguredSessionModelSelection,
  useModelsQuery,
  useProjectsView,
  useSessionQuery,
  useUpdateSessionModelMutation,
  useUpdateSessionProjectMutation,
  useSubmitUserActionMutation,
} from "@/features/chat";
import { useAppStore } from "@/shared/store/app-store";
import { UNBOUND_PROJECT_ID } from "@/shared/types/domain";
import type { ReasoningLevel } from "@/shared/types/ui";
import { ChatContextBar } from "@/widgets/chat-context-bar/ChatContextBar";
import { ChatSessionTimeline } from "./ChatSessionTimeline";
import type { SessionLocationState } from "./chatSessionLocation";
import {
  hasInitialSessionInput,
  useInitialSessionInput,
} from "./useInitialSessionInput";
import { useChatSessionAutoScroll } from "./useChatSessionAutoScroll";
import { useSessionCommands } from "./useSessionCommands";
import { useSessionPermissionMode } from "./useSessionPermissionMode";
import type { ModelRef } from "@talos/sdk-browser";

export function ChatSessionPage() {
  const navigate = useNavigate();
  const location = useLocation();
  const sessionId = useParams().sessionId ?? "";
  const locationState = location.state as SessionLocationState | null;
  const [draftModel, setDraftModel] = useState<ModelRef | undefined>(locationState?.initialModel);
  const [draftReasoningLevel, setDraftReasoningLevel] = useState<ReasoningLevel | undefined>(
    locationState?.initialReasoningLevel,
  );
  const [draftProjectId, setDraftProjectId] = useState<string | undefined>(
    locationState?.initialProjectId,
  );
  const { data: session } = useSessionQuery(
    sessionId,
    !hasInitialSessionInput(locationState),
  );
  const { data: modelCatalog } = useModelsQuery();
  const models = modelCatalog?.models;
  const projectsView = useProjectsView();
  const {
    blocks,
    draft,
    inputHistory,
    setDraft,
    sendTurn,
    sendText,
    isStreaming,
    isCancelling,
    contextCompactionStage,
    canRetryResponse,
    retryResponse,
    retryStream,
    showReconnectControls,
    stopResponse,
    streamError,
    sessionHistoryQuery,
    activeTurnId,
    pendingUserAction,
  } = useChatSession(sessionId, session?.message_count, session?.title);
  const { mutate: updateSessionModel } = useUpdateSessionModelMutation(sessionId);
  const updateProject = useUpdateSessionProjectMutation(sessionId);
  const submitUserAction = useSubmitUserActionMutation(sessionId);
  const connection = useAppStore((state) => state.connection);
  const setConnection = useAppStore((state) => state.setConnection);
  const projects = projectsView.projects;
  const sidebarOpen = useAppStore((state) => state.sidebarOpen);
  const activeProjectId = projectsView.activeProjectId;
  const setActiveProjectId = projectsView.setActiveProjectId;
  const activeSessionModelSelection = useConfiguredSessionModelSelection({
    catalog: modelCatalog,
    selection: session?.modelSelection,
    sessionId,
    updateSelection: updateSessionModel,
  });
  const currentModelRef = activeSessionModelSelection?.modelRef ?? draftModel ?? modelCatalog?.defaultModelSelection.modelRef;
  const currentModel = currentModelRef ? modelRefKey(currentModelRef) : undefined;
  const currentModelInfo = modelInfoForRef(models, currentModelRef);
  const currentReasoningLevel = activeSessionModelSelection
    ? reasoningLevelForEffort(activeSessionModelSelection.reasoningEffort)
    : draftReasoningLevel ?? (draftModel
        ? reasoningLevelForModel(models, draftModel)
        : reasoningLevelForEffort(modelCatalog?.defaultModelSelection.reasoningEffort ?? null));
  const permissionMode = useSessionPermissionMode({
    blocked: Boolean(isStreaming || pendingUserAction),
    initialMode: locationState?.initialPermissionMode,
    persistedMode: session?.effectiveMode,
    sessionExists: Boolean(session),
    sessionId,
  });
  const sessionCommands = useSessionCommands({
    contextCompactionStage,
    currentModel,
    isStreaming,
    sessionId,
  });
  const sessionHistoryError = sessionHistoryQuery.error;
  const shouldShowMessageList =
    blocks.length > 0 || (sessionHistoryQuery.isSuccess && !sessionHistoryError);
  const autoScroll = useChatSessionAutoScroll({ blocks, isStreaming, sessionId, streamError });

  const selectedProjectId = useMemo(() => {
    if (!session) {
      return draftProjectId ?? activeProjectId;
    }
    return session.project_id ?? UNBOUND_PROJECT_ID;
  }, [activeProjectId, draftProjectId, session]);
  const boundProject = useMemo(() => {
    if (!session?.project_id) {
      return undefined;
    }
    return projects.find((entry) => entry.id === session.project_id);
  }, [projects, session?.project_id]);
  useInitialSessionInput({
    locationPathname: location.pathname,
    locationState,
    models,
    navigate,
    projects,
    sendText,
    sessionId,
    setDraft,
  });

  if (!sessionId) {
    navigate("/");
    return null;
  }

  return (
    <div className="flex h-full w-full flex-col overflow-hidden">
      {sidebarOpen ? (
        <ChatContextBar
          loading={!session}
          onProjectClick={
            boundProject
              ? () => {
                  setActiveProjectId(boundProject.id);
                  navigate("/");
                }
              : undefined
          }
          projectName={boundProject?.name}
          sessionTitle={session?.title}
        />
      ) : null}
      <ChatSessionTimeline
        blocks={blocks}
        canRetryResponse={canRetryResponse}
        contentRef={autoScroll.contentRef}
        endRef={autoScroll.endRef}
        isCancelling={isCancelling}
        onUserActionSubmit={(response) => {
          if (!pendingUserAction || !activeTurnId) {
            return;
          }
          submitUserAction.mutate({
            turnId: activeTurnId,
            actionId: pendingUserAction.actionId,
            response,
          });
        }}
        onRetryResponse={retryResponse}
        onRetryStream={retryStream}
        onScroll={autoScroll.handleScroll}
        onStopResponse={stopResponse}
        sessionHistoryError={session ? sessionHistoryError : undefined}
        showMessageList={shouldShowMessageList}
        showReconnectControls={showReconnectControls}
        streamError={streamError}
        pendingUserAction={pendingUserAction}
        userActionDisabled={
          submitUserAction.isPending || pendingUserAction?.state !== "pending"
        }
        userActionError={submitUserAction.error instanceof Error
          ? submitUserAction.error.message
          : undefined}
        viewportRef={autoScroll.viewportRef}
      />
      <div className="mt-4 shrink-0">
        <div className="mx-auto w-full max-w-[920px] px-7 pb-5">
          <ChatComposer
            activeProjectId={selectedProjectId}
            disabled={false}
            draft={draft}
            inputHistory={inputHistory}
            isCancelling={isCancelling}
            commandNotice={sessionCommands.notice}
            contextCompactionStage={sessionCommands.compactionStage}
            isStreaming={isStreaming}
            permissionMode={permissionMode.value}
            permissionModeDisabled={permissionMode.disabled}
            models={modelPickerOptions(models)}
            initialAttachments={locationState?.initialAttachments}
            onCancel={() => void stopResponse()}
            onCommand={sessionCommands.execute}
            onDraftChange={setDraft}
            onModelChange={(model) => {
              if (sessionCommands.compactionStage) {
                return;
              }
              const modelRef = modelRefForKey(model);
              if (!modelRef) {
                return;
              }
              if (!session) {
                setDraftModel(modelRef);
                setDraftReasoningLevel(reasoningLevelForModel(models, modelRef));
                return;
              }
              const modelSelection = modelSelectionForRef(models, modelRef);
              if (modelSelection) {
                updateSessionModel(modelSelection);
              }
            }}
            onPermissionModeChange={permissionMode.onChange}
            onProjectChange={(projectId) => {
              if (sessionCommands.compactionStage) {
                return;
              }
              setActiveProjectId(projectId);
              if (!session) {
                setDraftProjectId(projectId);
                return;
              }
              const project = projects.find((entry) => entry.id === projectId);
              if (project || projectId === UNBOUND_PROJECT_ID) {
                updateProject.mutate(project);
              }
            }}
            onRuntimeModeChange={(mode) => {
              if (sessionCommands.compactionStage) {
                return;
              }
              if (mode === "local") {
                setConnection({ runtimeMode: mode });
              }
            }}
            onReasoningLevelChange={(level) => {
              if (sessionCommands.compactionStage) {
                return;
              }
              if (!session) {
                setDraftReasoningLevel(level);
                return;
              }
              const modelSelection = modelSelectionForRef(models, currentModelRef, level);
              if (modelSelection) {
                updateSessionModel(modelSelection);
              }
            }}
            onSend={(skills, attachments) => {
              const fileIds = attachments.map((file) => file.fileId);
              const fileOptions = fileIds.length > 0 ? { fileIds } : {};
              if (!session) {
                const selectedProject = projects.find((entry) => entry.id === selectedProjectId);
                return sendTurn({
                  modelSelection: modelSelectionForRef(
                    models,
                    currentModelRef,
                    currentReasoningLevel,
                  ),
                  project: selectedProject,
                  skills: skills.length > 0 ? skills : undefined,
                  userPermissionMode: permissionMode.value,
                  ...fileOptions,
                });
              }
              return sendTurn({
                skills: skills.length > 0 ? skills : undefined,
                ...fileOptions,
              });
            }}
            projects={projects}
            runtimeMode={connection.runtimeMode}
            reasoningLevel={currentReasoningLevel}
            reasoningEnabled={currentModelInfo?.reasoning ?? false}
            supportedAttachmentKinds={supportedAttachmentKinds(currentModelInfo)}
            selectedModel={currentModel}
            showProjectSelector={false}
            showRuntimeModeSelector={false}
          />
        </div>
      </div>
    </div>
  );
}
