import { useLocation, useNavigate } from "react-router-dom";

import {
  useDeleteProjectMutation,
  useDeleteSessionMutation,
  useProjectsView,
  useSessionsQuery,
  useUpdateProjectNameMutation,
} from "../hooks/useChatQueries";
import { useLogout } from "@/features/auth";
import { useAppStore } from "@/shared/store/app-store";
import { getEffectiveSessionStatus, isUnboundProject } from "@/shared/types/domain";
import { Sidebar } from "@/widgets/sidebar/Sidebar";
import type { SidebarProjectGroup } from "@/widgets/sidebar/SidebarProjectSection";
import type { SidebarSessionItem } from "@/widgets/sidebar/SidebarSessionRow";

export function SidebarContainer({ computer }: { computer?: import("@talos/sdk-core").AppProtocol.NodeInfo } = {}) {
  const location = useLocation();
  const navigate = useNavigate();
  const connection = useAppStore((state) => state.connection);
  const handleLogout = useLogout();
  const setSidebarOpen = useAppStore((state) => state.setSidebarOpen);
  const aliases = useAppStore((state) => state.sessionAliases);
  const statuses = useAppStore((state) => state.transientStatuses);
  const streams = useAppStore((state) => state.streams);
  const deleteSessionAlias = useAppStore((state) => state.deleteSessionAlias);
  const resetStream = useAppStore((state) => state.resetStream);
  const setSessionStatus = useAppStore((state) => state.setSessionStatus);
  const sessionsQuery = useSessionsQuery();
  const deleteSession = useDeleteSessionMutation();
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
      status: getEffectiveSessionStatus(statuses[sessionId], streams[sessionId]),
      model: session.modelSelection.modelRef.id,
      projectId: session.projectId ?? null,
      lastActivityAt: session.lastActivity,
      createdAt: session.createdAt,
    };
  });
  const projectIds = new Set(projects.map((project) => project.id));
  const chats = threads.filter(
    (thread) =>
      isUnboundProject(thread.projectId) ||
      (thread.projectId !== null && thread.projectId !== undefined && !projectIds.has(thread.projectId)),
  );
  const projectGroups: SidebarProjectGroup[] = projects
    .filter((project) => !isUnboundProject(project.id))
    .map((project) => ({
      project,
      sessions: threads.filter((thread) => thread.projectId === project.id),
    }));

  return (
    <Sidebar
      computer={computer}
      showAccount={false}
      chats={chats}
      connection={connection}
      currentPath={location.pathname}
      onCollapse={() => setSidebarOpen(false)}
      onCreateThread={async () => {
        navigate("/");
      }}
      onCreateProjectThread={(projectId) => {
        projectsView.setActiveProjectId(projectId);
        navigate("/");
      }}
      onRemoveProject={async (projectId) => {
        await deleteProject.mutateAsync(projectId);
      }}
      onDeleteSession={async (sessionId) => {
        await deleteSession.mutateAsync(sessionId);
        deleteSessionAlias(sessionId);
        resetStream(sessionId);
        setSessionStatus(sessionId, undefined);
        if (location.pathname === `/sessions/${sessionId}`) navigate("/");
      }}
      onRenameProject={async (projectId, name) => {
        await updateProjectName.mutateAsync({ projectId, name });
      }}
      onSelectThread={(sessionId) => navigate(`/sessions/${sessionId}`)}
      onLogout={handleLogout}
      projectActionPending={deleteProject.isPending || updateProjectName.isPending}
      sessionActionPending={deleteSession.isPending}
      projectGroups={projectGroups}
    />
  );
}
