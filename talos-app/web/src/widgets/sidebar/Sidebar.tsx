import { useEffect, useRef, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";

import { apiErrorMessage, userVisibleError } from "@/shared/i18n/apiErrorMessage";
import type { ProjectView } from "@/shared/types/ui";
import type { AccountType, AuthStatus } from "@/shared/types/account";
import { SidebarAccountMenu } from "./SidebarAccountMenu";
import { SidebarChatsSection } from "./SidebarChatsSection";
import { SidebarHomeEntry } from "./SidebarHomeEntry";
import { SidebarPinnedSection } from "./SidebarPinnedSection";
import { SidebarPrimaryNav } from "./SidebarPrimaryNav";
import { SidebarProjectSection, type SidebarProjectGroup } from "./SidebarProjectSection";
import type { SidebarSessionItem } from "./SidebarSessionRow";
import { SidebarSessionDeleteDialog } from "./SidebarSessionDeleteDialog";

type AuthConnectionState = {
  authStatus: AuthStatus;
  authRole: "user" | "admin";
  accountType: AccountType;
  authUsername: string;
  userId: string;
};

interface SidebarProps {
  computer?: import("@talos/sdk-core").AppProtocol.NodeInfo;
  computerEntry?: ReactNode;
  showAccount?: boolean;
  pinnedSessions?: SidebarSessionItem[];
  chats: SidebarSessionItem[];
  connection: AuthConnectionState;
  currentPath: string;
  projectGroups: SidebarProjectGroup[];
  onCreateProjectThread: (projectId: ProjectView["id"]) => void;
  onCreateThread: () => Promise<void> | void;
  onLogout: () => Promise<void> | void;
  onRemoveProject: (projectId: ProjectView["id"]) => Promise<void> | void;
  onRenameProject: (projectId: ProjectView["id"], name: string) => Promise<void> | void;
  onSelectThread: (sessionId: string) => void;
  onDeleteSession: (sessionId: string) => Promise<void> | void;
  onTogglePinSession?: (sessionId: string, pinned: boolean) => Promise<void> | void;
  pendingPinSessionIds?: ReadonlySet<string>;
  sessionActionPending?: boolean;
  projectActionPending?: boolean;
}

export function Sidebar({
  computer,
  computerEntry,
  showAccount = true,
  pinnedSessions = [],
  chats,
  connection,
  currentPath,
  projectGroups,
  onCreateProjectThread,
  onCreateThread,
  onLogout,
  onRemoveProject,
  onRenameProject,
  onSelectThread,
  onDeleteSession,
  onTogglePinSession,
  pendingPinSessionIds,
  projectActionPending,
  sessionActionPending = false,
}: SidebarProps) {
  const { t } = useTranslation(["sidebar", "errors"]);
  const [sessionPendingDelete, setSessionPendingDelete] = useState<SidebarSessionItem | null>(null);
  const [pinError, setPinError] = useState<ReturnType<typeof userVisibleError> | null>(null);
  const sidebarRef = useRef<HTMLElement | null>(null);
  const pendingPinFocusRef = useRef<string | null>(null);

  useEffect(() => {
    const sessionId = pendingPinFocusRef.current;
    if (!sessionId) {
      return;
    }
    const activeElement = document.activeElement;
    const focusLost =
      !activeElement ||
      activeElement === document.body ||
      !sidebarRef.current?.contains(activeElement);
    if (!focusLost) {
      pendingPinFocusRef.current = null;
      return;
    }
    const escapedId = typeof CSS !== "undefined" && typeof CSS.escape === "function"
      ? CSS.escape(sessionId)
      : sessionId.replaceAll('"', '\\"');
    const targetButton = sidebarRef.current?.querySelector<HTMLButtonElement>(
      `[data-sidebar-pin-button="${escapedId}"]`,
    );
    if (targetButton) {
      targetButton.focus();
      pendingPinFocusRef.current = null;
      return;
    }
    // Session moved into a collapsed project group; keep focus from jumping to body
    // only if it was already dropped, without forcing the project open.
    pendingPinFocusRef.current = null;
  }, [pinnedSessions, chats, projectGroups]);

  async function handleTogglePinSession(sessionId: string, pinned: boolean) {
    if (!onTogglePinSession || pendingPinSessionIds?.has(sessionId)) {
      return;
    }
    const active = document.activeElement;
    if (
      active instanceof Element &&
      active.getAttribute("data-sidebar-pin-button") === sessionId
    ) {
      pendingPinFocusRef.current = sessionId;
    } else {
      pendingPinFocusRef.current = null;
    }
    setPinError(null);
    try {
      await onTogglePinSession(sessionId, pinned);
    } catch (error) {
      setPinError(userVisibleError(error, t("sidebar:pinSessionFailed")));
    }
  }

  return (
    <aside
      className="sticky top-0 flex h-full w-full shrink-0 flex-col border-r border-border/80 bg-sidebar px-4 py-4"
      ref={sidebarRef}
    >
      <SidebarHomeEntry computer={computer} />
      <SidebarPrimaryNav onCreateThread={onCreateThread} computerEntry={computerEntry} />

      <div className="min-h-0 flex-1 overflow-y-auto">
        {pinError ? (
          <div
            className="mb-2 rounded-[var(--radius-control)] border border-danger/20 bg-danger/5 px-3 py-2 text-xs text-danger"
            role="alert"
          >
            {apiErrorMessage(pinError)}
          </div>
        ) : null}
        <SidebarPinnedSection
          currentPath={currentPath}
          onDeleteSession={setSessionPendingDelete}
          onSelectSession={onSelectThread}
          onTogglePinSession={onTogglePinSession ? handleTogglePinSession : undefined}
          pendingPinSessionIds={pendingPinSessionIds}
          sessions={pinnedSessions}
        />
        <SidebarProjectSection
          currentPath={currentPath}
          groups={projectGroups}
          onDeleteSession={setSessionPendingDelete}
          onCreateProjectThread={onCreateProjectThread}
          onRemoveProject={onRemoveProject}
          onRenameProject={onRenameProject}
          onSelectSession={onSelectThread}
          onTogglePinSession={onTogglePinSession ? handleTogglePinSession : undefined}
          pendingPinSessionIds={pendingPinSessionIds}
          projectActionPending={projectActionPending}
        />
        <SidebarChatsSection
          currentPath={currentPath}
          onDeleteSession={setSessionPendingDelete}
          onSelectSession={onSelectThread}
          onTogglePinSession={onTogglePinSession ? handleTogglePinSession : undefined}
          pendingPinSessionIds={pendingPinSessionIds}
          sessions={chats}
        />
      </div>

      {showAccount ? <SidebarAccountMenu
        accountType={connection.accountType}
        authStatus={connection.authStatus}
        authUsername={connection.authUsername}
        onLogout={onLogout}
      /> : null}
      {sessionPendingDelete ? (
        <SidebarSessionDeleteDialog
          deleting={sessionActionPending}
          onClose={() => setSessionPendingDelete(null)}
          onDelete={async () => {
            await onDeleteSession(sessionPendingDelete.id);
            setSessionPendingDelete(null);
          }}
          sessionTitle={sessionPendingDelete.label}
        />
      ) : null}
    </aside>
  );
}
