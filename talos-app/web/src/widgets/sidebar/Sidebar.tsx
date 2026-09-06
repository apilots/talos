import type { ProjectView } from "@/shared/types/ui";
import type { AccountType, AuthStatus } from "@/shared/types/account";
import { SidebarAccountMenu } from "./SidebarAccountMenu";
import { SidebarChatsSection } from "./SidebarChatsSection";
import { SidebarCollapseButton } from "./SidebarCollapseButton";
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
  chats: SidebarSessionItem[];
  connection: AuthConnectionState;
  currentPath: string;
  projectGroups: SidebarProjectGroup[];
  onCollapse: () => void;
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
  chats,
  connection,
  currentPath,
  projectGroups,
  onCollapse,
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
    <aside className="sticky top-0 flex h-screen w-full shrink-0 flex-col border-r border-border/80 bg-sidebar px-4 py-4">
      <div className="absolute right-4 top-4 z-20">
        <SidebarCollapseButton onCollapse={onCollapse} />
      </div>
      <SidebarHomeEntry />
      <SidebarPrimaryNav onCreateThread={onCreateThread} />

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

      <SidebarAccountMenu
        accountType={connection.accountType}
        authStatus={connection.authStatus}
        authUsername={connection.authUsername}
        onLogout={onLogout}
      />
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
import { useState } from "react";
