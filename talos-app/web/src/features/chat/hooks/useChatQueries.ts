import { useEffect } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import {
  cancelActiveTurn,
  clearSession,
  compactSession,
  createProject,
  deleteProject,
  deleteSession,
  getActiveTurn,
  getSessionHistory,
  recoverSessionLiveTail,
  getSession,
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
  activeTurn: (connectionKey: string, sessionId: string) =>
    [...chatKeys.all, connectionKey, "active-turn", sessionId] as const,
  sessionHistory: (connectionKey: string, sessionId: string) =>
    [...chatKeys.all, connectionKey, "session-history", sessionId] as const,
  liveRecovery: (connectionKey: string, sessionId: string, afterEventId?: string) =>
    [...chatKeys.all, connectionKey, "live-recovery", sessionId, afterEventId ?? "tail"] as const,
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

  const data = [...(query.data ?? [])]
    .sort((left, right) => right.last_activity - left.last_activity)
    .filter((session) => {
      if (threadFilter === "all") {
        return true;
      }
      const state = useAppStore.getState();
      const status = getEffectiveSessionStatus(
        state.transientStatuses[session.id],
        state.streams[session.id],
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
  const replaceProjects = useAppStore((state) => state.replaceProjects);
  const query = useQuery({
    queryKey: [...chatKeys.all, connectionKey, "projects"] as const,
    queryFn: listProjects,
  });

  useEffect(() => {
    if (!query.data) {
      return;
    }
    replaceProjects(query.data);
  }, [query.data, replaceProjects]);

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

export function useActiveTurnQuery(sessionId: string) {
  const connectionKey = useConnectionKey();
  return useQuery({
    queryKey: chatKeys.activeTurn(connectionKey, sessionId),
    queryFn: () => getActiveTurn(sessionId),
    enabled: Boolean(sessionId),
    refetchInterval: (query) =>
      query.state.data?.status === "running" ||
      query.state.data?.status === "waiting_user_action"
        ? 4_000
        : false,
  });
}

export function useLiveRecoveryQuery(
  sessionId: string,
  afterEventId?: string,
  enabled = true,
) {
  const connectionKey = useConnectionKey();
  return useQuery({
    queryKey: chatKeys.liveRecovery(connectionKey, sessionId, afterEventId),
    queryFn: () => recoverSessionLiveTail(sessionId, afterEventId),
    enabled: Boolean(sessionId) && enabled,
  });
}

export function useSessionHistoryQuery(sessionId: string) {
  const connectionKey = useConnectionKey();
  return useQuery({
    queryKey: chatKeys.sessionHistory(connectionKey, sessionId),
    queryFn: () => getSessionHistory(sessionId),
    enabled: Boolean(sessionId),
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

export function useDeleteProjectMutation() {
  const connectionKey = useConnectionKey();
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: deleteProject,
    onSuccess: async () => {
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
      queryClient.removeQueries({ queryKey: chatKeys.sessionHistory(connectionKey, sessionId) });
      queryClient.removeQueries({ queryKey: chatKeys.liveRecovery(connectionKey, sessionId) });
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
        await queryClient.invalidateQueries({ queryKey: chatKeys.sessionHistory(connectionKey, sessionId) });
        await queryClient.invalidateQueries({ queryKey: chatKeys.liveRecovery(connectionKey, sessionId) });
      },
    }),
    clear: useMutation({
      mutationFn: () => clearSession(sessionId),
      onSuccess: async () => {
        await queryClient.invalidateQueries({ queryKey: chatKeys.sessions(connectionKey) });
        await queryClient.invalidateQueries({ queryKey: chatKeys.session(connectionKey, sessionId) });
        await queryClient.invalidateQueries({ queryKey: chatKeys.sessionHistory(connectionKey, sessionId) });
        await queryClient.invalidateQueries({ queryKey: chatKeys.liveRecovery(connectionKey, sessionId) });
      },
    }),
  };
}
