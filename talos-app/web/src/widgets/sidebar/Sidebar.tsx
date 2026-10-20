import type { ProjectView } from "@/shared/types/ui";
import type { AccountType, AuthStatus } from "@/shared/types/account";
import { SidebarAccountMenu } from "./SidebarAccountMenu";
import { SidebarChatsSection } from "./SidebarChatsSection";
import { SidebarHomeEntry } from "./SidebarHomeEntry";
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
  sessionActionPending?: boolean;
  projectActionPending?: boolean;
}

export function Sidebar({
  computer,
  computerEntry,
  showAccount = true,
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
  projectActionPending,
  sessionActionPending = false,
}: SidebarProps) {
  const [sessionPendingDelete, setSessionPendingDelete] = useState<SidebarSessionItem | null>(null);

  return (
    <aside className="sticky top-0 flex h-full w-full shrink-0 flex-col border-r border-border/80 bg-sidebar px-4 py-4">
      <SidebarHomeEntry computer={computer} />
      <SidebarPrimaryNav onCreateThread={onCreateThread} computerEntry={computerEntry} />

      <div className="min-h-0 flex-1 overflow-y-auto">
        <SidebarProjectSection
          currentPath={currentPath}
          groups={projectGroups}
          onDeleteSession={setSessionPendingDelete}
          onCreateProjectThread={onCreateProjectThread}
          onRemoveProject={onRemoveProject}
          onRenameProject={onRenameProject}
          onSelectSession={onSelectThread}
          projectActionPending={projectActionPending}
        />
        <SidebarChatsSection
          currentPath={currentPath}
          onDeleteSession={setSessionPendingDelete}
          onSelectSession={onSelectThread}
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
import { useState, type ReactNode } from "react";
