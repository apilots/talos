import { useEffect } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import {
  cancelActiveTurn,
  clearSession,
  compactSession,
  createProject,
  createSession,
  deleteSession,
  getSession,
  getSessionTurns,
  listModels,
  listProjects,
  listSessions,
  updateSessionModel,
  updateSessionProject,
} from "../api";
import { useAppStore } from "@/shared/store/app-store";
import { getEffectiveSessionStatus } from "@/shared/types/domain";

export const chatKeys = {
  all: ["talos"] as const,
  models: (connectionKey: string) => [...chatKeys.all, connectionKey, "models"] as const,
  sessions: (connectionKey: string) => [...chatKeys.all, connectionKey, "sessions"] as const,
  session: (connectionKey: string, sessionId: string) =>
    [...chatKeys.all, connectionKey, "session", sessionId] as const,
  turns: (connectionKey: string, sessionId: string) =>
    [...chatKeys.all, connectionKey, "turns", sessionId] as const,
};

export function useConnectionKey() {
  return useAppStore((state) =>
    `${state.connection.serverUrl}|${state.connection.authToken}|${state.connection.userId}`,
  );
}

export function useModelsQuery() {
  const connectionKey = useConnectionKey();
  return useQuery({
    queryKey: chatKeys.models(connectionKey),
    queryFn: listModels,
    staleTime: 60_000,
  });
}

export function useSessionsQuery() {
  const connectionKey = useConnectionKey();
  const threadFilter = useAppStore((state) => state.threadFilter);
  const query = useQuery({
    queryKey: chatKeys.sessions(connectionKey),
    queryFn: listSessions,
  });

  const data = (query.data ?? [])
    .sort((left, right) => right.state.last_activity - left.state.last_activity)
    .filter((session) => {
      if (threadFilter === "all") {
        return true;
      }
      const state = useAppStore.getState();
      const status = getEffectiveSessionStatus(
        state.transientStatuses[session.context.session_id],
        state.streams[session.context.session_id],
      );
      return status === threadFilter;
    });

  return {
    ...query,
    data,
  };
}

export function useProjectsQuery() {
  const connectionKey = useConnectionKey();
  const mergeProjects = useAppStore((state) => state.mergeProjects);
  const query = useQuery({
    queryKey: [...chatKeys.all, connectionKey, "projects"] as const,
    queryFn: listProjects,
  });

  useEffect(() => {
    if (!query.data) {
      return;
    }
    mergeProjects(query.data);
  }, [mergeProjects, query.data]);

  return query;
}

export function useSessionQuery(sessionId: string) {
  const connectionKey = useConnectionKey();
  return useQuery({
    queryKey: chatKeys.session(connectionKey, sessionId),
    queryFn: () => getSession(sessionId),
    enabled: Boolean(sessionId),
  });
}

export function useSessionTurnsQuery(sessionId: string) {
  const connectionKey = useConnectionKey();
  return useQuery({
    queryKey: chatKeys.turns(connectionKey, sessionId),
    queryFn: () => getSessionTurns(sessionId),
    enabled: Boolean(sessionId),
  });
}

export function useCreateSessionMutation() {
  const connectionKey = useConnectionKey();
  const queryClient = useQueryClient();
  const userId = useAppStore((state) => state.connection.userId);
  const activeProjectId = useAppStore((state) => state.activeProjectId);
  const projects = useAppStore((state) => state.projects);
  return useMutation({
    mutationFn: (model: string | undefined) =>
      createSession(
        userId,
        model,
        projects.find((project) => project.id === activeProjectId),
      ),
    onSuccess: async (session) => {
      await queryClient.invalidateQueries({ queryKey: chatKeys.sessions(connectionKey) });
      await queryClient.setQueryData(chatKeys.session(connectionKey, session.context.session_id), session);
    },
  });
}

export function useUpdateSessionProjectMutation(sessionId: string) {
  const connectionKey = useConnectionKey();
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: updateSessionProject.bind(null, sessionId),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: chatKeys.sessions(connectionKey) });
      await queryClient.invalidateQueries({ queryKey: chatKeys.session(connectionKey, sessionId) });
    },
  });
}

export function useCreateProjectMutation() {
  const connectionKey = useConnectionKey();
  const queryClient = useQueryClient();
  const mergeProjects = useAppStore((state) => state.mergeProjects);
  return useMutation({
    mutationFn: createProject,
    onSuccess: async (project) => {
      mergeProjects([project], project.id);
      await queryClient.invalidateQueries({ queryKey: [...chatKeys.all, connectionKey, "projects"] });
    },
  });
}

export function useUpdateSessionModelMutation(sessionId: string) {
  const connectionKey = useConnectionKey();
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: updateSessionModel.bind(null, sessionId),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: chatKeys.sessions(connectionKey) });
      await queryClient.invalidateQueries({ queryKey: chatKeys.session(connectionKey, sessionId) });
    },
  });
}

export function useDeleteSessionMutation() {
  const connectionKey = useConnectionKey();
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: deleteSession,
    onSuccess: async (_, sessionId) => {
      await queryClient.invalidateQueries({ queryKey: chatKeys.sessions(connectionKey) });
      queryClient.removeQueries({ queryKey: chatKeys.session(connectionKey, sessionId) });
      queryClient.removeQueries({ queryKey: chatKeys.turns(connectionKey, sessionId) });
    },
  });
}

export function useSessionActions(sessionId: string) {
  const connectionKey = useConnectionKey();
  const queryClient = useQueryClient();
  return {
    cancelTurn: useMutation({
      mutationFn: () => cancelActiveTurn(sessionId),
      onSuccess: async () => {
        await queryClient.invalidateQueries({ queryKey: chatKeys.sessions(connectionKey) });
      },
    }),
    compact: useMutation({
      mutationFn: () => compactSession(sessionId),
      onSuccess: async () => {
        await queryClient.invalidateQueries({ queryKey: chatKeys.sessions(connectionKey) });
        await queryClient.invalidateQueries({ queryKey: chatKeys.session(connectionKey, sessionId) });
        await queryClient.invalidateQueries({ queryKey: chatKeys.turns(connectionKey, sessionId) });
      },
    }),
    clear: useMutation({
      mutationFn: () => clearSession(sessionId),
      onSuccess: async () => {
        await queryClient.invalidateQueries({ queryKey: chatKeys.sessions(connectionKey) });
        await queryClient.invalidateQueries({ queryKey: chatKeys.session(connectionKey, sessionId) });
        await queryClient.invalidateQueries({ queryKey: chatKeys.turns(connectionKey, sessionId) });
      },
    }),
  };
}
