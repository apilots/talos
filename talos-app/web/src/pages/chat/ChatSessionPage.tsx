import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useLocation, useNavigate, useParams } from "react-router-dom";

import {
  ChatComposer,
  useChatSession,
  useModelsQuery,
  useProjectsView,
  useSessionQuery,
  useUpdateSessionModelMutation,
  useUpdateSessionProjectMutation,
} from "@/features/chat";
import { useAppStore } from "@/shared/store/app-store";
import { getSessionModel, UNBOUND_PROJECT_ID } from "@/shared/types/domain";
import type { ProjectView } from "@/shared/types/ui";
import { MessageList } from "@/widgets/message-list/MessageList";
import { ChatContextBar } from "@/widgets/chat-context-bar/ChatContextBar";
import { ChatSessionReconnectPanel } from "./ChatSessionReconnectPanel";
import { ChatSessionErrorPanel, ChatSessionMessageActions } from "./ChatSessionStatusPanels";
import { useChatSessionAutoScroll } from "./useChatSessionAutoScroll";
import type { ModelInfo, ModelRef, SkillSuggestion } from "@talos/sdk-browser";

interface SessionLocationState {
  initialPrompt?: string;
  initialModel?: string;
  initialProjectId?: string;
  initialProject?: ProjectView;
  initialSkills?: SkillSuggestion[];
}

function selectedSkillsOption(skills?: SkillSuggestion[]) {
  return skills && skills.length > 0 ? skills : undefined;
}

function modelRefForId(models: ModelInfo[] | undefined, modelId: string | undefined): ModelRef | undefined {
  if (!modelId) {
    return undefined;
  }
  const model = models?.find((entry) => entry.id === modelId);
  return model ? { provider: model.provider, id: model.id } : undefined;
}

export function ChatSessionPage() {
  const { t } = useTranslation(["chat"]);
  const params = useParams();
  const navigate = useNavigate();
  const location = useLocation();
  const sessionId = params.sessionId ?? "";
  const initialPromptHandledRef = useRef(false);
  const mountedRef = useRef(false);
  const activeSessionIdRef = useRef(sessionId);
  const locationState = location.state as SessionLocationState | null;
  const [draftModel, setDraftModel] = useState<string | undefined>(locationState?.initialModel);
  const [draftProjectId, setDraftProjectId] = useState<string | undefined>(
    locationState?.initialProjectId,
  );
  const { data: session } = useSessionQuery(sessionId);
  const { data: models } = useModelsQuery();
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
    canRetryResponse,
    retryResponse,
    retryStream,
    showReconnectControls,
    stopResponse,
    streamError,
    sessionHistoryQuery,
  } = useChatSession(sessionId, session?.message_count, session?.title);
  const updateModel = useUpdateSessionModelMutation(sessionId);
  const updateProject = useUpdateSessionProjectMutation(sessionId);
  const connection = useAppStore((state) => state.connection);
  const setConnection = useAppStore((state) => state.setConnection);
  const projects = projectsView.projects;
  const sidebarOpen = useAppStore((state) => state.sidebarOpen);
  const activeProjectId = projectsView.activeProjectId;
  const setActiveProjectId = projectsView.setActiveProjectId;
  const currentModel = session ? getSessionModel(session) : draftModel ?? models?.[0]?.id;
  const sessionHistoryError = sessionHistoryQuery.error;
  const shouldShowMessageList =
    blocks.length > 0 || (sessionHistoryQuery.isSuccess && !sessionHistoryError);
  const autoScroll = useChatSessionAutoScroll({ blocks, sessionId, streamError });

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);

  useEffect(() => {
    activeSessionIdRef.current = sessionId;
  }, [sessionId]);

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
  useEffect(() => {
    const initialPrompt = locationState?.initialPrompt?.trim();
    if (!initialPrompt || initialPromptHandledRef.current) {
      return;
    }
    initialPromptHandledRef.current = true;
    autoScroll.stickToBottomRef.current = true;
    window.requestAnimationFrame(autoScroll.scrollToBottom);
    const initialProject =
      locationState?.initialProject ??
      projects.find((entry) => entry.id === locationState?.initialProjectId);
    void sendText(initialPrompt, {
      modelRef: modelRefForId(models, locationState?.initialModel),
      project: initialProject,
      skills: selectedSkillsOption(locationState?.initialSkills),
    })
      .catch(() => undefined)
      .finally(() => {
        if (mountedRef.current && activeSessionIdRef.current === sessionId) {
          navigate(location.pathname, { replace: true, state: null });
        }
      });
  }, [
    autoScroll.scrollToBottom,
    autoScroll.stickToBottomRef,
    location.pathname,
    locationState?.initialModel,
    locationState?.initialProject,
    locationState?.initialProjectId,
    locationState?.initialPrompt,
    locationState?.initialSkills,
    models,
    navigate,
    projects,
    sendText,
    sessionId,
  ]);

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
      <div
        className="min-h-0 flex-1 overflow-y-auto"
        onScroll={(event) => autoScroll.handleScroll(event.currentTarget)}
        ref={autoScroll.viewportRef}
      >
        <div className="mx-auto w-full max-w-[920px] space-y-4 px-7 pb-2 pt-5" ref={autoScroll.contentRef}>
          {shouldShowMessageList ? (
            <MessageList
              actions={<ChatSessionMessageActions />}
              blocks={blocks}
              emptyDetail={t("chat:emptyReadyDetail")}
              emptyTitle={t("chat:emptyReadyTitle")}
            />
          ) : null}
          {streamError ? (
            <ChatSessionErrorPanel title={streamError} />
          ) : null}
          {sessionHistoryError ? (
            <ChatSessionErrorPanel
              detail={sessionHistoryError instanceof Error
                ? sessionHistoryError.message
                : t("chat:unknownSessionHistoryError")}
              title={t("chat:sessionHistoryFailed")}
            />
          ) : null}
          {showReconnectControls ? (
            <ChatSessionReconnectPanel
              canRetryResponse={canRetryResponse}
              isCancelling={isCancelling}
              onRetryResponse={() => void retryResponse()}
              onRetryStream={() => void retryStream()}
              onStopResponse={() => void stopResponse()}
            />
          ) : null}
          <div aria-hidden="true" ref={autoScroll.endRef} />
        </div>
      </div>
      <div className="mt-4 shrink-0">
        <div className="mx-auto w-full max-w-[920px] px-7 pb-5">
          <ChatComposer
            activeProjectId={selectedProjectId}
            disabled={false}
            draft={draft}
            inputHistory={inputHistory}
            isCancelling={isCancelling}
            isStreaming={isStreaming}
            models={(models ?? []).map((model) => ({ id: model.id, name: model.name }))}
            onCancel={() => void stopResponse()}
            onDraftChange={setDraft}
            onModelChange={(model) => {
              if (!session) {
                setDraftModel(model);
                return;
              }
              const modelRef = modelRefForId(models, model);
              if (modelRef) {
                updateModel.mutate(modelRef);
              }
            }}
            onProjectChange={(projectId) => {
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
              if (mode === "local") {
                setConnection({ runtimeMode: mode });
              }
            }}
            onSend={(skills) => {
              autoScroll.stickToBottomRef.current = true;
              window.requestAnimationFrame(autoScroll.scrollToBottom);
              if (!session) {
                const selectedProject = projects.find((entry) => entry.id === selectedProjectId);
                return sendTurn({
                  modelRef: modelRefForId(models, currentModel),
                  project: selectedProject,
                  skills: selectedSkillsOption(skills),
                });
              }
              return sendTurn({ skills: selectedSkillsOption(skills) });
            }}
            projects={projects}
            runtimeMode={connection.runtimeMode}
            selectedModel={currentModel}
            showProjectSelector={false}
            showRuntimeModeSelector={false}
          />
        </div>
      </div>
    </div>
  );
}
