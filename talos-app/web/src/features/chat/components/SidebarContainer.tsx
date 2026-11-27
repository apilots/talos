import { useState, type ReactNode } from "react";
import { useLocation, useNavigate } from "react-router-dom";

import {
  useDeleteProjectMutation,
  useDeleteSessionMutation,
  useProjectsView,
  useSessionsQuery,
  useUpdateProjectNameMutation,
} from "../hooks/useChatQueries";
import { useToggleSessionPinMutation } from "../hooks/useToggleSessionPinMutation";
import { useLogout } from "@/features/auth";
import { useAppStore } from "@/shared/store/app-store";
import { getEffectiveSessionStatus, isUnboundProject } from "@/shared/types/domain";
import { Sidebar } from "@/widgets/sidebar/Sidebar";
import type { SidebarProjectGroup } from "@/widgets/sidebar/SidebarProjectSection";
import type { SidebarSessionItem } from "@/widgets/sidebar/SidebarSessionRow";

export function SidebarContainer({ computer, computerEntry }: { computer?: import("@talos/sdk-core").AppProtocol.NodeInfo; computerEntry?: ReactNode } = {}) {
  const location = useLocation();
  const navigate = useNavigate();
  const connection = useAppStore((state) => state.connection);
  const handleLogout = useLogout();
  const aliases = useAppStore((state) => state.sessionAliases);
  const statuses = useAppStore((state) => state.transientStatuses);
  const streams = useAppStore((state) => state.streams);
  const deleteSessionAlias = useAppStore((state) => state.deleteSessionAlias);
  const resetStream = useAppStore((state) => state.resetStream);
  const setSessionStatus = useAppStore((state) => state.setSessionStatus);
  const sessionsQuery = useSessionsQuery();
  const deleteSession = useDeleteSessionMutation();
  const toggleSessionPin = useToggleSessionPinMutation();
  const [pendingPinSessionIds, setPendingPinSessionIds] = useState<Set<string>>(() => new Set());
  const deleteProject = useDeleteProjectMutation();
  const updateProjectName = useUpdateProjectNameMutation();
  const projectsView = useProjectsView();
  const projects = projectsView.projects;

  const sessions = sessionsQuery.data ?? [];
  const threads: SidebarSessionItem[] = sessions.map((session) => {
    const sessionId = session.id;
    return {
      id: sessionId,
      label: aliases[sessionId] ?? session.title,
      // Only the selected Session has an event observer. Background rows use
      // the server list snapshot, never its abandoned live-response cache.
      status: location.pathname === `/sessions/${sessionId}`
        ? getEffectiveSessionStatus(statuses[sessionId], streams[sessionId])
        : session.hasActiveTurn ? "running" : "done",
      model: session.modelSelection.modelRef.id,
      projectId: session.projectId ?? null,
      pinned: Boolean(session.pinned),
      lastActivityAt: session.lastActivity,
      createdAt: session.createdAt,
    };
  });
  const pinnedSessions = threads.filter((thread) => thread.pinned);
  const unpinnedThreads = threads.filter((thread) => !thread.pinned);
  const projectIds = new Set(projects.map((project) => project.id));
  const chats = unpinnedThreads.filter(
    (thread) =>
      isUnboundProject(thread.projectId) ||
      (thread.projectId !== null && thread.projectId !== undefined && !projectIds.has(thread.projectId)),
  );
  const projectGroups: SidebarProjectGroup[] = projects
    .filter((project) => !isUnboundProject(project.id))
    .map((project) => ({
      project,
      sessions: unpinnedThreads.filter((thread) => thread.projectId === project.id),
    }));

  return (
    <Sidebar
      computer={computer}
      computerEntry={computerEntry}
      showAccount={false}
      pinnedSessions={pinnedSessions}
      chats={chats}
      connection={connection}
      currentPath={location.pathname}
      onCreateThread={() => {
        void navigate("/");
      }}
      onCreateProjectThread={(projectId) => {
        projectsView.setActiveProjectId(projectId);
        void navigate("/");
      }}
      onRemoveProject={async (projectId) => {
        await deleteProject.mutateAsync(projectId);
      }}
      onDeleteSession={async (sessionId) => {
        await deleteSession.mutateAsync(sessionId);
        deleteSessionAlias(sessionId);
        resetStream(sessionId);
        setSessionStatus(sessionId, undefined);
        if (location.pathname === `/sessions/${sessionId}`) void navigate("/");
      }}
      onTogglePinSession={async (sessionId, pinned) => {
        setPendingPinSessionIds((current) => new Set(current).add(sessionId));
        try {
          await toggleSessionPin.mutateAsync({ sessionId, pinned });
        } finally {
          setPendingPinSessionIds((current) => {
            const next = new Set(current);
            next.delete(sessionId);
            return next;
          });
        }
      }}
      pendingPinSessionIds={pendingPinSessionIds}
      onRenameProject={async (projectId, name) => {
        await updateProjectName.mutateAsync({ projectId, name });
      }}
      onSelectThread={(sessionId) => void navigate(`/sessions/${sessionId}`)}
      onLogout={handleLogout}
      projectActionPending={deleteProject.isPending || updateProjectName.isPending}
      sessionActionPending={deleteSession.isPending}
      projectGroups={projectGroups}
    />
  );
}
