import { userVisibleErrorMessage } from "@/shared/i18n/apiErrorMessage";
import { useEffect, useState } from "react";
import { useLocation, useNavigate, useParams } from "react-router-dom";

import {
  ChatComposer,
  SessionClearConfirmationDialog,
  useChatSession,
  useModelsQuery,
  useProjectsView,
  useSessionQuery,
  useSubmitUserActionMutation,
} from "@/features/chat";
import { useComputer, useComputers, useProductScope } from "@/features/computers";
import { useAppStore } from "@/shared/store/app-store";
import { ChatContextBar } from "@/widgets/chat-context-bar/ChatContextBar";
import { ChatSessionTimeline } from "./ChatSessionTimeline";
import type { SessionLocationState } from "./chatSessionLocation";
import {
  hasInitialSessionInput,
  useInitialSessionInput,
} from "./useInitialSessionInput";
import { useChatSessionAutoScroll } from "./useChatSessionAutoScroll";
import { useChatSessionComposer } from "./useChatSessionComposer";

export function ChatSessionPage() {
  const navigate = useNavigate();
  const location = useLocation();
  const sessionId = useParams().sessionId ?? "";
  const locationState = location.state as SessionLocationState | null;
  const [pendingInitialSessionId, setPendingInitialSessionId] = useState<string>();
  const sessionQuery = useSessionQuery(
    sessionId,
    !hasInitialSessionInput(locationState) && pendingInitialSessionId !== sessionId,
  );
  const session = sessionQuery.data;
  const computers = useComputers();
  const scope = useProductScope();
  const selectComputer = useAppStore((state) => state.selectComputer);
  const nodeId = session?.nodeId ?? locationState?.initialNodeId ?? computers.selectedId;
  const target = useComputer(nodeId);
  useEffect(() => { if (session?.nodeId) selectComputer(scope, session.nodeId); }, [session?.nodeId, sessionId, scope, selectComputer]);
  const { data: modelCatalog } = useModelsQuery(nodeId);
  const models = modelCatalog?.models;
  const projectsView = useProjectsView();
  const chat = useChatSession(sessionId, session?.messageCount);
  const {
    blocks,
    setDraft,
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
    activeTurnId,
    pendingUserAction,
  } = chat;
  const submitUserAction = useSubmitUserActionMutation(sessionId);
  const projects = projectsView.projects;
  const sidebarOpen = useAppStore((state) => state.sidebarOpen);
  const sessionHistoryError = sessionHistoryQuery.error;
  const shouldShowMessageList =
    blocks.length > 0 || (sessionHistoryQuery.isSuccess && !sessionHistoryError);
  const autoScroll = useChatSessionAutoScroll({ blocks, isStreaming, sessionId, streamError });

  const sessionComposer = useChatSessionComposer({
    chat,
    locationState,
    modelCatalog,
    projectsView,
    session,
    sessionId,
  });
  useInitialSessionInput({
    onPendingSessionChange: setPendingInitialSessionId,
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
      {sessionQuery.error && !locationState ? <p role="alert" className="px-7 py-3 text-sm text-danger">{userVisibleErrorMessage(sessionQuery.error, "Unable to load this session.")}</p> : null}
      {sidebarOpen ? (
        <ChatContextBar
          loading={!session}
          onProjectClick={
            sessionComposer.boundProject
              ? () => {
                  projectsView.setActiveProjectId(sessionComposer.boundProject!.id);
                  navigate("/");
                }
              : undefined
          }
          projectName={sessionComposer.boundProject?.name}
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
        userActionError={submitUserAction.error
          ? userVisibleErrorMessage(submitUserAction.error, "Failed to submit the response.")
          : undefined}
        viewportRef={autoScroll.viewportRef}
      />
      <div className="mt-4 shrink-0">
        <div className="mx-auto w-full max-w-[920px] px-7 pb-5">
          <ChatComposer {...sessionComposer.props} disabled={!target.data?.available || (sessionQuery.isPending && !locationState && pendingInitialSessionId !== sessionId)} />
        </div>
      </div>
      <SessionClearConfirmationDialog {...sessionComposer.clearConfirmation} />
    </div>
  );
}
