import type { ComponentProps, RefObject } from "react";
import { useTranslation } from "react-i18next";

import { MessageList } from "@/widgets/message-list/MessageList";
import { UserActionPanel } from "@/features/chat";
import { ChatSessionReconnectPanel } from "./ChatSessionReconnectPanel";
import { ChatSessionErrorPanel, ChatSessionMessageActions } from "./ChatSessionStatusPanels";

interface ChatSessionTimelineProps {
  blocks: ComponentProps<typeof MessageList>["blocks"];
  canRetryResponse: boolean;
  contentRef: RefObject<HTMLDivElement | null>;
  endRef: RefObject<HTMLDivElement | null>;
  isCancelling: boolean;
  pendingUserAction?: ComponentProps<typeof MessageList>["pendingUserAction"];
  userActionDisabled?: boolean;
  userActionError?: string;
  onUserActionSubmit?: ComponentProps<typeof UserActionPanel>["onSubmit"];
  onRetryResponse: () => Promise<void> | void;
  onRetryStream: () => Promise<void> | void;
  onScroll: (element: HTMLDivElement) => void;
  onStopResponse: () => Promise<void> | void;
  sessionHistoryError?: unknown;
  showMessageList: boolean;
  showReconnectControls: boolean;
  streamError?: string;
  viewportRef: RefObject<HTMLDivElement | null>;
}

export function ChatSessionTimeline({
  blocks,
  canRetryResponse,
  contentRef,
  endRef,
  isCancelling,
  pendingUserAction,
  userActionDisabled,
  userActionError,
  onUserActionSubmit,
  onRetryResponse,
  onRetryStream,
  onScroll,
  onStopResponse,
  sessionHistoryError,
  showMessageList,
  showReconnectControls,
  streamError,
  viewportRef,
}: ChatSessionTimelineProps) {
  const { t } = useTranslation("chat");

  return (
    <div
      className="min-h-0 flex-1 overflow-y-auto"
      onScroll={(event) => onScroll(event.currentTarget)}
      ref={viewportRef}
    >
      <div className="mx-auto w-full max-w-[920px] space-y-4 px-7 pb-2 pt-5" ref={contentRef}>
        {showMessageList ? (
          <MessageList
            actions={<ChatSessionMessageActions />}
            blocks={blocks}
            emptyDetail={t("emptyReadyDetail")}
            emptyTitle={t("emptyReadyTitle")}
            pendingUserAction={pendingUserAction}
            renderUserAction={onUserActionSubmit
              ? (action) => (
                  <UserActionPanel
                    action={action}
                    disabled={userActionDisabled}
                    error={userActionError}
                    onSubmit={onUserActionSubmit}
                  />
                )
              : undefined}
          />
        ) : null}
        {streamError ? <ChatSessionErrorPanel title={streamError} /> : null}
        {sessionHistoryError ? (
          <ChatSessionErrorPanel
            detail={sessionHistoryError instanceof Error
              ? sessionHistoryError.message
              : t("unknownSessionHistoryError")}
            title={t("sessionHistoryFailed")}
          />
        ) : null}
        {showReconnectControls ? (
          <ChatSessionReconnectPanel
            canRetryResponse={canRetryResponse}
            isCancelling={isCancelling}
            onRetryResponse={() => void onRetryResponse()}
            onRetryStream={() => void onRetryStream()}
            onStopResponse={() => void onStopResponse()}
          />
        ) : null}
        <div aria-hidden="true" ref={endRef} />
      </div>
    </div>
  );
}
