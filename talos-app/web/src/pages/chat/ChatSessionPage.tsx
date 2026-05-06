import { useEffect, useLayoutEffect, useMemo, useRef } from "react";
import { useTranslation } from "react-i18next";
import { useLocation, useNavigate, useParams } from "react-router-dom";

import {
  ChatComposer,
  useUpdateSessionApprovalModeMutation,
  useChatSession,
  useModelsQuery,
  useProjectsQuery,
  useSessionQuery,
  useUpdateSessionModelMutation,
  useUpdateSessionProjectMutation,
} from "@/features/chat";
import { useAppStore } from "@/shared/store/app-store";
import { getSessionApprovalMode, getSessionModel, UNBOUND_PROJECT_ID } from "@/shared/types/domain";
import { MessageList } from "@/widgets/message-list/MessageList";
import { ChatContextBar } from "@/widgets/chat-context-bar/ChatContextBar";
import { Panel } from "@/shared/ui/Panel";
import { Button } from "@/shared/ui/Button";

interface SessionLocationState {
  initialPrompt?: string;
}

export function ChatSessionPage() {
  const { t } = useTranslation(["chat"]);
  const params = useParams();
  const navigate = useNavigate();
  const location = useLocation();
  const sessionId = params.sessionId ?? "";
  const initialPromptHandledRef = useRef(false);
  const messageViewportRef = useRef<HTMLDivElement | null>(null);
  const messageContentRef = useRef<HTMLDivElement | null>(null);
  const messageEndRef = useRef<HTMLDivElement | null>(null);
  const stickToBottomRef = useRef(true);
  const locationState = location.state as SessionLocationState | null;

  const { data: session } = useSessionQuery(sessionId);
  const { data: models } = useModelsQuery();
  useProjectsQuery();
  const {
    blocks,
    draft,
    setDraft,
    sendMessage,
    sendText,
    submitApproval,
    isStreaming,
    isCancelling,
    pendingApproval,
    canRetryResponse,
    retryResponse,
    retryStream,
    showReconnectControls,
    stopResponse,
    streamError,
  } = useChatSession(sessionId, session?.state.message_count, session?.title);
  const updateModel = useUpdateSessionModelMutation(sessionId);
  const updateProject = useUpdateSessionProjectMutation(sessionId);
  const updateApprovalMode = useUpdateSessionApprovalModeMutation(sessionId);
  const connection = useAppStore((state) => state.connection);
  const setConnection = useAppStore((state) => state.setConnection);
  const projects = useAppStore((state) => state.projects);
  const activeProjectId = useAppStore((state) => state.activeProjectId);
  const setActiveProjectId = useAppStore((state) => state.setActiveProjectId);
  const approvalMode = useAppStore(
    (state) =>
      state.sessionApprovalModes[sessionId]
      ?? getSessionApprovalMode(session)
      ?? state.connection.defaultApprovalMode,
  );
  const setSessionApprovalMode = useAppStore((state) => state.setSessionApprovalMode);
  const currentModel = session ? getSessionModel(session) : models?.[0]?.id;

  const scrollToBottom = () => {
    const viewport = messageViewportRef.current;
    if (!viewport) {
      return;
    }
    viewport.scrollTop = viewport.scrollHeight;
    messageEndRef.current?.scrollIntoView({ block: "end" });
  };

  const selectedProjectId = useMemo(() => {
    if (!session) {
      return activeProjectId;
    }
    return session.project_id ?? UNBOUND_PROJECT_ID;
  }, [activeProjectId, projects, session]);
  const boundProject = useMemo(() => {
    if (!session?.project_id) {
      return undefined;
    }
    return projects.find((entry) => entry.id === session.project_id);
  }, [projects, session?.project_id]);
  useEffect(() => {
    const initialPrompt = locationState?.initialPrompt?.trim();
    if (!session || !initialPrompt || initialPromptHandledRef.current) {
      return;
    }
    initialPromptHandledRef.current = true;
    stickToBottomRef.current = true;
    window.requestAnimationFrame(scrollToBottom);
    void sendText(initialPrompt).finally(() => {
      navigate(location.pathname, { replace: true, state: null });
    });
  }, [location.pathname, locationState?.initialPrompt, navigate, sendText, session]);

  useEffect(() => {
    if (!session) {
      return;
    }
    setSessionApprovalMode(sessionId, getSessionApprovalMode(session));
  }, [session, sessionId, setSessionApprovalMode]);

  useEffect(() => {
    stickToBottomRef.current = true;
  }, [sessionId]);

  useEffect(() => {
    const content = messageContentRef.current;
    if (!content || typeof ResizeObserver === "undefined") {
      return;
    }

    const observer = new ResizeObserver(() => {
      if (!stickToBottomRef.current) {
        return;
      }

      const viewport = messageViewportRef.current;
      if (!viewport) {
        return;
      }

      viewport.scrollTop = viewport.scrollHeight;
      messageEndRef.current?.scrollIntoView({ block: "end" });
    });

    observer.observe(content);
    return () => observer.disconnect();
  }, []);

  useLayoutEffect(() => {
    const viewport = messageViewportRef.current;
    if (!viewport || !stickToBottomRef.current) {
      return;
    }

    const scrollToBottom = () => {
      viewport.scrollTop = viewport.scrollHeight;
      messageEndRef.current?.scrollIntoView({ block: "end" });
    };

    scrollToBottom();
    const frameId = window.requestAnimationFrame(scrollToBottom);
    const timeoutId = window.setTimeout(scrollToBottom, 120);
    return () => {
      window.cancelAnimationFrame(frameId);
      window.clearTimeout(timeoutId);
    };
  }, [blocks, pendingApproval, streamError, sessionId]);

  if (!sessionId) {
    navigate("/");
    return null;
  }

  return (
    <div className="flex h-full w-full flex-col overflow-hidden">
      <ChatContextBar
        loading={!session}
        onProjectClick={
          boundProject
            ? () => {
                navigate(`/projects/${boundProject.id}`);
              }
            : undefined
        }
        projectName={boundProject?.name}
        sessionTitle={session?.title}
      />
      <div
        className="min-h-0 flex-1 overflow-y-auto"
        onScroll={(event) => {
          const viewport = event.currentTarget;
          const distanceToBottom =
            viewport.scrollHeight - viewport.scrollTop - viewport.clientHeight;
          stickToBottomRef.current = distanceToBottom < 80;
        }}
        ref={messageViewportRef}
      >
        <div className="mx-auto w-full max-w-[920px] space-y-4 px-7 pb-2 pt-5" ref={messageContentRef}>
          <MessageList
            actions={
              <>
                <Button onClick={() => navigate("/files")} variant="secondary">{t("chat:openFiles")}</Button>
                <Button onClick={() => navigate("/memory")} variant="secondary">{t("chat:openMemory")}</Button>
                <Button onClick={() => navigate("/settings")} variant="secondary">{t("chat:openSettings")}</Button>
              </>
            }
            blocks={blocks}
            emptyDetail={t("chat:emptyReadyDetail")}
            emptyTitle={t("chat:emptyReadyTitle")}
          />
          {pendingApproval ? (
            <Panel className="rounded-[1.5rem] border-warning/30 bg-warning/5 p-4">
              <div className="flex flex-wrap items-center justify-between gap-3">
                <div>
                  <div className="text-xs uppercase tracking-[0.16em] text-warning">{t("chat:approvalRequired")}</div>
                  <div className="mt-2 text-base font-medium">{pendingApproval.toolName}</div>
                  <p className="mt-2 text-sm text-muted">{pendingApproval.prompt}</p>
                </div>
                <div className="flex items-center gap-3">
                  <Button onClick={() => void submitApproval(false)} variant="danger">{t("common:reject")}</Button>
                  <Button onClick={() => void submitApproval(true)}>{t("common:approve")}</Button>
                </div>
              </div>
            </Panel>
          ) : null}
          {streamError ? (
            <Panel className="rounded-[1.5rem] border-danger/30 bg-danger/5 p-4">
              <div className="text-sm text-danger">{streamError}</div>
            </Panel>
          ) : null}
          {showReconnectControls ? (
            <Panel className="rounded-xl border-border/70 bg-surface/70 px-3 py-2">
              <div className="flex flex-wrap items-center justify-between gap-2">
                <div className="min-w-0 text-sm text-muted">
                  <span className="font-medium text-ink">{t("chat:liveResponseInterrupted")}</span>
                  <span className="ml-2">{t("chat:interruptedDetail")}</span>
                </div>
                <div className="flex shrink-0 items-center gap-2">
                  <Button
                    className="px-2.5 py-1.5 text-xs"
                    disabled={isCancelling}
                    onClick={() => void retryStream()}
                    variant="secondary"
                  >
                    {t("common:refreshNow")}
                  </Button>
                  {canRetryResponse ? (
                    <Button
                      className="px-2.5 py-1.5 text-xs"
                      disabled={isCancelling}
                      onClick={() => void retryResponse()}
                      variant="secondary"
                    >
                      {t("common:retryResponse")}
                    </Button>
                  ) : null}
                  <Button
                    className="px-2.5 py-1.5 text-xs"
                    disabled={isCancelling}
                    onClick={() => void stopResponse()}
                    variant="ghost"
                  >
                    {t("common:stopResponse")}
                  </Button>
                </div>
              </div>
            </Panel>
          ) : null}
          <div aria-hidden="true" ref={messageEndRef} />
        </div>
      </div>
      <div className="mt-4 shrink-0">
        <div className="mx-auto w-full max-w-[920px] px-7 pb-5">
          <ChatComposer
            activeProjectId={selectedProjectId}
            approvalMode={approvalMode}
            disabled={!session}
            draft={draft}
            isCancelling={isCancelling}
            isStreaming={isStreaming}
            models={(models ?? []).map((model) => ({ id: model.id, name: model.name }))}
            onApprovalModeChange={(mode) => {
              setSessionApprovalMode(sessionId, mode);
              updateApprovalMode.mutate(mode);
            }}
            onCancel={() => void stopResponse()}
            onDraftChange={setDraft}
            onModelChange={(model) => updateModel.mutate(model)}
            onProjectChange={(projectId) => {
              setActiveProjectId(projectId);
              const project = projects.find((entry) => entry.id === projectId);
              if (project || projectId === UNBOUND_PROJECT_ID) {
                updateProject.mutate(project);
              }
            }}
            onRuntimeModeChange={(mode) => {
              if (mode === "cloud") {
                setConnection({ runtimeMode: mode });
              }
            }}
            onSend={() => {
              stickToBottomRef.current = true;
              window.requestAnimationFrame(scrollToBottom);
              return sendMessage();
            }}
            projects={projects}
            runtimeMode={connection.runtimeMode}
            selectedModel={currentModel}
            showProjectSelector={false}
          />
        </div>
      </div>
    </div>
  );
}
