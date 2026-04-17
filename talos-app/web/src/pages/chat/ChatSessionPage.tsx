import { useEffect, useLayoutEffect, useMemo, useRef } from "react";
import { useTranslation } from "react-i18next";
import { useLocation, useNavigate, useParams } from "react-router-dom";

import {
  ChatComposer,
  useChatSession,
  useModelsQuery,
  useSessionQuery,
  useUpdateSessionModelMutation,
  useUpdateSessionProjectMutation,
} from "@/features/chat";
import { useAppStore } from "@/shared/store/app-store";
import { getSessionModel, resolveProjectRootPath } from "@/shared/types/domain";
import { MessageList } from "@/widgets/message-list/MessageList";
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
    retryStream,
    stopResponse,
    streamConnectionState,
    streamError,
  } = useChatSession(sessionId, session?.state.message_count);
  const updateModel = useUpdateSessionModelMutation(sessionId);
  const updateProject = useUpdateSessionProjectMutation(sessionId);
  const connection = useAppStore((state) => state.connection);
  const setConnection = useAppStore((state) => state.setConnection);
  const projects = useAppStore((state) => state.projects);
  const activeProjectId = useAppStore((state) => state.activeProjectId);
  const setActiveProjectId = useAppStore((state) => state.setActiveProjectId);
  const approvalMode = useAppStore(
    (state) => state.sessionApprovalModes[sessionId] ?? state.connection.defaultApprovalMode,
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
    const matchedProject = projects.find((project) => {
      const path = resolveProjectRootPath(project.root);
      const sessionPath = session.context.project_root ?? __TALOS_DEFAULT_PROJECT_ROOT__;
      return path === sessionPath;
    });
    return matchedProject?.id ?? activeProjectId;
  }, [activeProjectId, projects, session]);
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
    <div className="mx-auto flex h-full w-full max-w-[920px] flex-col overflow-hidden px-7 py-5">
      <div
        className="min-h-0 flex-1 space-y-4 overflow-y-auto pr-2"
        onScroll={(event) => {
          const viewport = event.currentTarget;
          const distanceToBottom =
            viewport.scrollHeight - viewport.scrollTop - viewport.clientHeight;
          stickToBottomRef.current = distanceToBottom < 80;
        }}
        ref={messageViewportRef}
      >
        <div className="space-y-4 pb-2" ref={messageContentRef}>
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
          {streamConnectionState === "detached" || streamConnectionState === "reconnecting" ? (
            <Panel className="rounded-[1.5rem] border-brand/20 bg-brand/5 p-4">
              <div className="flex flex-wrap items-center justify-between gap-3">
                <div>
                  <div className="text-xs uppercase tracking-[0.16em] text-brand">
                    {streamConnectionState === "reconnecting" ? t("chat:reconnecting") : t("chat:connectionLost")}
                  </div>
                  <div className="mt-2 text-base font-medium text-ink">
                    {streamConnectionState === "reconnecting"
                      ? t("chat:refreshingSessionHistory")
                      : t("chat:liveResponseInterrupted")}
                  </div>
                  <p className="mt-2 max-w-2xl text-sm text-muted">
                    {streamConnectionState === "reconnecting"
                      ? t("chat:refreshingDetail")
                      : t("chat:interruptedDetail")}
                  </p>
                </div>
                <div className="flex items-center gap-3">
                  <Button
                    disabled={streamConnectionState === "reconnecting" || isCancelling}
                    onClick={() => void retryStream()}
                    variant="secondary"
                  >
                    {t("common:retryNow")}
                  </Button>
                  <Button disabled={isCancelling} onClick={() => void stopResponse()} variant="danger">{t("common:stopResponse")}</Button>
                </div>
              </div>
            </Panel>
          ) : null}
          <div aria-hidden="true" ref={messageEndRef} />
        </div>
      </div>
      <div className="mt-4 shrink-0">
          <ChatComposer
            activeProjectId={selectedProjectId}
            approvalMode={approvalMode}
            disabled={!session}
            draft={draft}
            isCancelling={isCancelling}
            isStreaming={isStreaming}
            models={(models ?? []).map((model) => ({ id: model.id, name: model.name }))}
            onApprovalModeChange={(mode) => setSessionApprovalMode(sessionId, mode)}
            onCancel={() => void stopResponse()}
            onDraftChange={setDraft}
            onModelChange={(model) => updateModel.mutate(model)}
            onProjectChange={(projectId) => {
              setActiveProjectId(projectId);
              const project = projects.find((entry) => entry.id === projectId);
              if (project) {
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
            compact
          />
      </div>
    </div>
  );
}
