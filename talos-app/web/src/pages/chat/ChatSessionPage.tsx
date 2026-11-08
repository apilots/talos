import { AppProtocol, TalosApiError } from "@talos/sdk-browser";
import { TaloPanel } from "@/features/computers";
import { useTranslation } from "react-i18next";
import { SessionTracingPanel, TraceTabs } from "@/features/tracing";
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
import {
  useComputer,
  useComputers,
  useProductScope,
} from "@/features/computers";
import { useAppStore } from "@/shared/store/app-store";
import { ChatSessionTimeline } from "./ChatSessionTimeline";
import type { SessionLocationState } from "./chatSessionLocation";
import {
  hasInitialSessionInput,
  useInitialSessionInput,
} from "./useInitialSessionInput";
import { useChatSessionAutoScroll } from "./useChatSessionAutoScroll";
import { useChatSessionComposer } from "./useChatSessionComposer";

export function ChatSessionPage() {
  return <ChatSessionContent sessionId={useParams().sessionId ?? ""} />;
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
  const location = useLocation();
  const locationState = talo ? null : location.state as SessionLocationState | null;
  const [pendingInitialSessionId, setPendingInitialSessionId] =
    useState<string>();
  const [restoredAutomationMode, setRestoredAutomationMode] = useState<{ scope: string; sessionId: string; mode: "goal" | "loop" }>();
  const sessionQuery = useSessionQuery(
    sessionId,
    !hasInitialSessionInput(locationState) &&
      pendingInitialSessionId !== sessionId,
  );
  const session = sessionQuery.data;
  const computers = useComputers();
  const scope = useProductScope();
  const selectComputer = useAppStore((state) => state.selectComputer);
  const nodeId =
    talo?.nodeId ?? session?.nodeId ?? locationState?.initialNodeId ?? computers.selectedId;
  const target = useComputer(nodeId);
  const workerPhase = talo?.phase ?? target.data?.managedWorkerPhase;
  const sleepingTalo = workerPhase === "sleeping";
  const ready = talo ? workerPhase === "ready" : target.data?.available;
  const firstTaloTurn = talo && sessionQuery.error instanceof TalosApiError &&
    sessionQuery.error.code === AppProtocol.ErrorCode.ResourceNotFound;
  useEffect(() => {
    const computer = talo?.nodeId ?? session?.nodeId;
    if (computer) selectComputer(scope, computer);
  }, [talo?.nodeId, session?.nodeId, sessionId, scope, selectComputer]);
  const { data: modelCatalog } = useModelsQuery(nodeId);
  const models = modelCatalog?.models;
  const projectsView = useProjectsView(nodeId);
  const { refetch: refetchProjects } = projectsView;
  useEffect(() => {
    // Another client may have created this Project while the sidebar stayed mounted.
    if (session?.projectId) void refetchProjects();
  }, [session?.projectId, session?.nodeId, refetchProjects]);
  const chat = useChatSession(sessionId, session?.messageCount, workerPhase);
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
  const sessionHistoryError = sessionHistoryQuery.error;
  const shouldShowMessageList =
    Boolean(firstTaloTurn) || blocks.length > 0 ||
    (sessionHistoryQuery.isSuccess && !sessionHistoryError);
  const autoScroll = useChatSessionAutoScroll({
    blocks,
    isStreaming,
    sessionId,
    streamError,
  });

  const sessionComposer = useChatSessionComposer({
    chat,
    locationState,
    modelCatalog,
    projectsView,
    session,
    sessionId,
    nodeId,
    sleepingTalo,
    taloConversation: Boolean(talo),
    restoredAutomationMode: restoredAutomationMode?.scope === scope && restoredAutomationMode.sessionId === sessionId ? restoredAutomationMode.mode : undefined,
    onRestoredAutomationModeConsumed: () => setRestoredAutomationMode(undefined),
  });
  useInitialSessionInput({
    onPendingSessionChange: setPendingInitialSessionId,
    onRestoreAutomationMode: (mode) => setRestoredAutomationMode({ scope, sessionId, mode }),
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
      {sessionQuery.error && !locationState && !firstTaloTurn ? (
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
            disabled={
              (!ready && !sleepingTalo) ||
              (sessionQuery.isPending &&
                !talo && !locationState &&
                pendingInitialSessionId !== sessionId)
            }
          />
        </div>
      </div>
      <SessionClearConfirmationDialog {...sessionComposer.clearConfirmation} />
    </div>
  );
}
