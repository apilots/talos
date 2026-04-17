import type { TalosSession } from "@talos/sdk";
import { useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";

import { chatKeys, listSessionsByProject, useConnectionKey, useProjectsQuery } from "@/features/chat";
import { useAppStore } from "@/shared/store/app-store";
import { getEffectiveSessionStatus, getSessionModel } from "@/shared/types/domain";

export function filterProjectSessions(sessions: TalosSession[], query: string) {
  const normalizedQuery = query.trim().toLowerCase();
  return sessions
    .slice()
    .sort((left, right) => right.state.last_activity - left.state.last_activity)
    .filter((session) => {
      if (!normalizedQuery) {
        return true;
      }
      return [session.title, getSessionModel(session)].some((field) =>
        field.toLowerCase().includes(normalizedQuery),
      );
    });
}

export function useProjectSessionsPageState(projectId: string) {
  const connectionKey = useConnectionKey();
  const projectsQuery = useProjectsQuery();
  const [query, setQuery] = useState("");
  const statuses = useAppStore((state) => state.transientStatuses);
  const streams = useAppStore((state) => state.streams);

  const sessionsQuery = useQuery({
    queryKey: [...chatKeys.all, connectionKey, "projectSessions", projectId] as const,
    queryFn: () => listSessionsByProject(projectId),
    enabled: Boolean(projectId),
  });

  const project = useMemo(
    () => (projectsQuery.data ?? []).find((entry) => entry.id === projectId),
    [projectId, projectsQuery.data],
  );

  const filteredSessions = useMemo(
    () => filterProjectSessions(sessionsQuery.data ?? [], query),
    [query, sessionsQuery.data],
  );

  const sessionRows = useMemo(
    () =>
      filteredSessions.map((session) => {
        const sessionId = session.context.session_id;
        return {
          id: sessionId,
          lastActivityAt: session.state.last_activity,
          model: getSessionModel(session),
          status: getEffectiveSessionStatus(statuses[sessionId], streams[sessionId]),
          title: session.title,
        };
      }),
    [filteredSessions, statuses, streams],
  );

  const notFound = !projectsQuery.isLoading && !projectsQuery.isError && !project;

  return {
    notFound,
    project,
    projectsQuery,
    query,
    sessionRows,
    sessionsQuery,
    setQuery,
  };
}
