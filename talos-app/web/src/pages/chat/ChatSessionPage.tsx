import { AppProtocol, TalosApiError } from "@talos/sdk-browser";
import { useQuery } from "@tanstack/react-query";
import { TaloPanel } from "@/features/computers";
import { useTranslation } from "react-i18next";
import { SessionTracingPanel, TraceTabs } from "@/features/tracing";
import { userVisibleErrorMessage } from "@/shared/i18n/apiErrorMessage";
import { useEffect, useState } from "react";
import { useNavigate, useParams } from "react-router-dom";

import {
  ChatComposer,
  SessionClearConfirmationDialog,
  useChatSession,
  useModelsQuery,
  useProjectsView,
  useSessionQuery,
  useSubmitUserActionMutation,
} from "@/features/chat";
import {
  useComputer,
  useManagedWorkerStatus,
  useProductScope,
} from "@/features/computers";
import { useAppStore } from "@/shared/store/app-store";
import { sessionMessageSendPolicyOptions } from "@/shared/api/session-message-send-policy";
import { isDesktopRuntime } from "@/shared/api/desktop-client";
import { isServiceUnavailable } from "@/shared/api/is-service-unavailable";
import { AttachmentSessionContext } from "@/shared/api/attachment-session";
import { ChatSessionTimeline } from "./ChatSessionTimeline";
import { useChatSessionAutoScroll } from "./useChatSessionAutoScroll";
import { useChatSessionComposer } from "./useChatSessionComposer";

export function ChatSessionPage() {
  const sessionId = useParams().sessionId ?? "";
  return <ChatSessionContent key={sessionId} sessionId={sessionId} />;
}

export function ChatSessionContent({ sessionId, talo }: {
  sessionId: string;
  talo?: AppProtocol.ManagedWorkerInfo;
}) {
  const { t: tracingText } = useTranslation("tracing");
  const { t: computerText } = useTranslation("computers");
  const [view, setView] = useState<"conversation" | "tracing">("conversation");
  const [tracingOpened, setTracingOpened] = useState(false);
  const changeView = (next: "conversation" | "tracing") => {
    if (next === "tracing") setTracingOpened(true);
    setView(next);
  };
  const navigate = useNavigate();
  const sessionQuery = useSessionQuery(sessionId);
  const session = sessionQuery.data;
  const missingSession = !session && sessionQuery.error instanceof TalosApiError &&
    sessionQuery.error.code === AppProtocol.ErrorCode.ResourceNotFound;
  const checkReservation = Boolean(!talo && missingSession);
  const { status } = useManagedWorkerStatus(checkReservation);
  const instance = talo ?? status.data?.instance;
  const reservedTalo = instance?.conversationSessionId === sessionId ? instance : undefined;
  const firstTaloTurn = Boolean(missingSession && reservedTalo);
  const scope = useProductScope();
  const selectComputer = useAppStore((state) => state.selectComputer);
  const nodeId = session?.nodeId ?? reservedTalo?.nodeId;
  const { data: computer, error: computerError } = useComputer(nodeId);
  const workerPhase = reservedTalo?.phase ?? computer?.managedWorkerPhase ?? (instance?.nodeId === nodeId ? instance?.phase : undefined);
  const ready = workerPhase == null ? computer?.available : workerPhase === "ready";
  const canSend = Boolean((ready || workerPhase === "sleeping") &&
    ((session && !sessionQuery.error) || firstTaloTurn));
  useEffect(() => {
    const computer = session?.nodeId ?? reservedTalo?.nodeId;
    if (computer) selectComputer(scope, computer);
  }, [reservedTalo?.nodeId, session?.nodeId, sessionId, scope, selectComputer]);
  const { data: modelCatalog } = useModelsQuery(nodeId);
  const computerOffline = computer?.status !== "disabled" &&
    (computerError ? isServiceUnavailable(computerError) : computer?.available === false);
  const stageOfflineAttachments = Boolean(!canSend && isDesktopRuntime() && session?.sessionKind === "standard" &&
    !sessionQuery.error && computerOffline);
  const projectsView = useProjectsView(nodeId);
  const { refetch: refetchProjects } = projectsView;
  useEffect(() => {
    // Another client may have created this Project while the sidebar stayed mounted.
    if (session?.projectId) void refetchProjects();
  }, [session?.projectId, session?.nodeId, refetchProjects]);
  const { data: sendPolicy } = useQuery({
    ...sessionMessageSendPolicyOptions(scope, nodeId),
    enabled: Boolean(nodeId && ready),
  });
  const chat = useChatSession(sessionId, session?.messageCount, workerPhase, sendPolicy);
  const {
    blocks,
    isStreaming,
    isCancelling,
    canRetryResponse,
    retryResponse,
    retryStream,
    showReconnectControls,
    streamConnectionState,
    stopResponse,
    streamError,
    sessionHistoryQuery,
    activeTurnId,
    pendingUserAction,
  } = chat;
  const submitUserAction = useSubmitUserActionMutation(sessionId);
  const sessionHistoryError = sessionHistoryQuery.error;
  const shouldShowMessageList =
    firstTaloTurn || blocks.length > 0 ||
    (sessionHistoryQuery.isSuccess && !sessionHistoryError);
  const autoScroll = useChatSessionAutoScroll({
    blocks,
    isStreaming,
    sessionId,
    streamError,
  });

  const sessionComposer = useChatSessionComposer({
    chat,
    modelCatalog,
    projectsView,
    session,
    sessionId,
    nodeId,
    managedTalo: workerPhase != null,
    canSend,
    stageOfflineAttachments,
    taloConversation: Boolean(reservedTalo),
  });

  if (!sessionId) {
    navigate("/");
    return null;
  }

  const content = (
    <div className="flex h-full w-full flex-col overflow-hidden">
      {sessionQuery.error && !firstTaloTurn ? (
        <p role="alert" className="px-7 py-3 text-sm text-danger">
          {userVisibleErrorMessage(
            sessionQuery.error,
            "Unable to load this session.",
          )}
        </p>
      ) : null}
      {talo ? (
        <header className="flex shrink-0 items-center justify-between gap-2 border-b border-border px-7 py-2">
          <h1 className="text-sm font-semibold text-ink">{computerText("talo.title")}</h1>
          <div className="flex flex-wrap items-center justify-end gap-2">
            {talo.diskQuota === "unsupported" ? <p className="text-xs text-muted">{computerText("talo.quotaUnsupported")}</p> : null}
            <TaloPanel desktopButton />
          </div>
        </header>
      ) : <TraceTabs
        className="session-view-tabs"
        label={tracingText("title")}
        value={view}
        onValueChange={changeView}
        tabs={(["conversation", "tracing"] as const).map((tab) => ({
          value: tab,
          id: `session-view-${tab}`,
          controls: `session-panel-${tab}`,
          label: tracingText(tab === "conversation" ? "conversation" : "tab"),
        }))}
      />}
      <div
        id="session-panel-conversation"
        role={talo ? undefined : "tabpanel"}
        aria-labelledby={talo ? undefined : "session-view-conversation"}
        className={
          view === "conversation" ? "flex min-h-0 flex-1 flex-col" : "hidden"
        }
      >
        <ChatSessionTimeline
          blocks={blocks}
          canRetryResponse={canRetryResponse}
          contentRef={autoScroll.contentRef}
          endRef={autoScroll.endRef}
          isCancelling={isCancelling}
          isReconnecting={streamConnectionState === "reconnecting"}
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
          userActionError={
            submitUserAction.error
              ? userVisibleErrorMessage(
                  submitUserAction.error,
                  "Failed to submit the response.",
                )
              : undefined
          }
          viewportRef={autoScroll.viewportRef}
        />
      </div>
      {!talo ? <div
        id="session-panel-tracing"
        role="tabpanel"
        aria-labelledby="session-view-tracing"
        className={
          view === "tracing" ? "flex min-h-0 flex-1 flex-col" : "hidden"
        }
      >
        {tracingOpened ? (
          <SessionTracingPanel
            key={sessionId}
            sessionId={sessionId}
            visible={view === "tracing"}
            history={sessionHistoryQuery.data}
          />
        ) : null}
      </div> : null}
      <div className={view === "conversation" ? "mt-4 shrink-0" : "hidden"}>
        <div className="mx-auto w-full max-w-[920px] px-7 pb-5">
          <ChatComposer
            key={JSON.stringify([scope, sessionId])}
            {...sessionComposer.props}
          />
        </div>
      </div>
      <SessionClearConfirmationDialog {...sessionComposer.clearConfirmation} />
    </div>
  );
  // History can arrive before session metadata. A new ordinary draft has neither.
  const attachmentSessionId = reservedTalo || session || blocks.length > 0 ? sessionId : undefined;
  return <AttachmentSessionContext.Provider value={attachmentSessionId}>{content}</AttachmentSessionContext.Provider>;
}
