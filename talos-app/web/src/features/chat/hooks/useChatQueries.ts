import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type {
  ModelSelection,
  SessionInfo,
  UserActionResponse,
  UserPermissionMode,
} from "@talos/sdk-browser";

import {
  cancelActiveTurn,
  clearSession,
  compactSession,
  createProject,
  deleteProject,
  deleteSession,
  getActiveTurn,
  getSessionHistory,
  submitTurnInput,
  getSession,
  listModels,
  listProjects,
  listSessions,
  updateProjectName,
  updateSessionModel,
  updateSessionPermissionMode,
} from "../api";
import { useAppStore } from "@/shared/store/app-store";
import { normalizeProjects } from "@/shared/store/ui-slice";
import type { ProjectView } from "@/shared/types/ui";

export const chatKeys = {
  all: ["talos"] as const,
  models: (connectionKey: string) => [...chatKeys.all, connectionKey, "models"] as const,
  sessions: (connectionKey: string) => [...chatKeys.all, connectionKey, "sessions"] as const,
  projects: (connectionKey: string) => [...chatKeys.all, connectionKey, "projects"] as const,
  session: (connectionKey: string, sessionId: string) =>
    [...chatKeys.all, connectionKey, "session", sessionId] as const,
  activeTurn: (connectionKey: string, sessionId: string) =>
    [...chatKeys.all, connectionKey, "active-turn", sessionId] as const,
  sessionHistory: (connectionKey: string, sessionId: string) =>
    [...chatKeys.all, connectionKey, "session-history", sessionId] as const,
};

export function useConnectionKey() {
  return useAppStore((state) =>
    `${state.connection.authToken}|${state.connection.userId}`,
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
  const query = useQuery({
    queryKey: chatKeys.sessions(connectionKey),
    queryFn: listSessions,
  });

  const data = [...(query.data ?? [])].sort(
    (left, right) => right.lastActivity - left.lastActivity,
  );

  return {
    ...query,
    data,
  };
}

export function useProjectsQuery() {
  const connectionKey = useConnectionKey();
  return useQuery({
    queryKey: chatKeys.projects(connectionKey),
    queryFn: listProjects,
  });
}

export function useProjectsView() {
  const query = useProjectsQuery();
  const activeProjectId = useAppStore((state) => state.activeProjectId);
  const setActiveProjectId = useAppStore((state) => state.setActiveProjectId);
  const projects = normalizeProjects(query.data);

  return {
    ...query,
    activeProjectId,
    projects,
    setActiveProjectId,
  };
}

export function useSessionQuery(sessionId: string, enabled = true) {
  const connectionKey = useConnectionKey();
  return useQuery({
    queryKey: chatKeys.session(connectionKey, sessionId),
    queryFn: () => getSession(sessionId),
    enabled: Boolean(sessionId) && enabled,
  });
}

export function useActiveTurnQuery(sessionId: string, enabled = true) {
  const connectionKey = useConnectionKey();
  return useQuery({
    queryKey: chatKeys.activeTurn(connectionKey, sessionId),
    queryFn: () => getActiveTurn(sessionId),
    enabled: Boolean(sessionId) && enabled,
    // Discover queue/remote starts while the idle Session has no event subscription.
    refetchInterval: 4_000,
  });
}

export function useSessionHistoryQuery(sessionId: string, enabled = true) {
  const connectionKey = useConnectionKey();
  return useQuery({
    queryKey: chatKeys.sessionHistory(connectionKey, sessionId),
    queryFn: () => getSessionHistory(sessionId),
    enabled: Boolean(sessionId) && enabled,
    retry: false,
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
      queryClient.setQueryData<ProjectView[]>(chatKeys.projects(connectionKey), (current) =>
        normalizeProjects([...(current ?? []), project]),
      );
      await queryClient.invalidateQueries({ queryKey: chatKeys.projects(connectionKey) });
    },
  });
}

export function useDeleteProjectMutation() {
  const connectionKey = useConnectionKey();
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: deleteProject,
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: chatKeys.projects(connectionKey) });
    },
  });
}

export function useUpdateProjectNameMutation() {
  const connectionKey = useConnectionKey();
  const queryClient = useQueryClient();
  const mergeProjects = useAppStore((state) => state.mergeProjects);
  return useMutation({
    mutationFn: ({ projectId, name }: { projectId: string; name: string }) =>
      updateProjectName(projectId, name),
    onSuccess: async (project) => {
      mergeProjects([project], project.id);
      queryClient.setQueryData<ProjectView[]>(chatKeys.projects(connectionKey), (current) =>
        normalizeProjects([...(current ?? []), project]),
      );
      await queryClient.invalidateQueries({ queryKey: chatKeys.projects(connectionKey) });
    },
  });
}

export function useUpdateSessionModelMutation(sessionId: string) {
  const connectionKey = useConnectionKey();
  const queryClient = useQueryClient();
  const sessionKey = chatKeys.session(connectionKey, sessionId);
  const mutationKey = ["session-model", connectionKey, sessionId] as const;
  return useMutation({
    mutationKey,
    scope: { id: `session-model:${connectionKey}:${sessionId}` },
    mutationFn: (modelSelection) => updateSessionModel(sessionId, modelSelection),
    onMutate: async (modelSelection: ModelSelection) => {
      await queryClient.cancelQueries({ queryKey: sessionKey });
      const previousSession = queryClient.getQueryData<SessionInfo>(sessionKey);
      queryClient.setQueryData<SessionInfo>(sessionKey, (current) =>
        current ? { ...current, modelSelection } : current,
      );
      return { previousSession };
    },
    onError: (_error, modelSelection, context) => {
      if (context?.previousSession) {
        queryClient.setQueryData<SessionInfo>(sessionKey, (current) => {
          const currentSelection = current?.modelSelection;
          const failedSelectionIsCurrent =
            currentSelection?.modelRef.provider === modelSelection.modelRef.provider &&
            currentSelection.modelRef.id === modelSelection.modelRef.id &&
            currentSelection.reasoningEffort === modelSelection.reasoningEffort;
          return failedSelectionIsCurrent ? context.previousSession : current;
        });
      }
    },
    onSettled: async () => {
      if (queryClient.isMutating({ mutationKey }) > 1) {
        return;
      }
      await queryClient.invalidateQueries({ queryKey: chatKeys.sessions(connectionKey) });
      await queryClient.invalidateQueries({ queryKey: sessionKey });
    },
  });
}

export function useUpdateSessionPermissionModeMutation(sessionId: string) {
  const connectionKey = useConnectionKey();
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (mode: UserPermissionMode) => updateSessionPermissionMode(sessionId, mode),
    onSuccess: (session) => {
      queryClient.setQueryData<SessionInfo>(
        chatKeys.session(connectionKey, sessionId),
        session,
      );
    },
  });
}

export function useSubmitUserActionMutation(sessionId: string) {
  const connectionKey = useConnectionKey();
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (request: { turnId: string; actionId: string; response: UserActionResponse }) =>
      submitTurnInput({
        identity: { sessionId, turnId: request.turnId },
        input: {
          type: "userActionResponse",
          actionId: request.actionId,
          response: request.response,
        },
        clientRequestId: null,
      }),
    onSuccess: async (_, request) => {
      const state = useAppStore.getState();
      const pending = state.streams[sessionId]?.pendingUserAction;
      if (pending?.actionId === request.actionId) {
        state.setPendingUserAction(sessionId, {
          ...pending,
          state: "resuming",
        });
      }
      await queryClient.invalidateQueries({
        queryKey: chatKeys.activeTurn(connectionKey, sessionId),
      });
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
    },
  });
}

export function useSessionActions(sessionId: string) {
  const connectionKey = useConnectionKey();
  const queryClient = useQueryClient();
  const applySessionResult = async (session: SessionInfo) => {
    queryClient.setQueryData<SessionInfo>(
      chatKeys.session(connectionKey, sessionId),
      session,
    );
    await queryClient.invalidateQueries({ queryKey: chatKeys.sessions(connectionKey) });
    await queryClient.invalidateQueries({ queryKey: chatKeys.sessionHistory(connectionKey, sessionId) });
  };
  return {
    cancelTurn: useMutation({
      mutationFn: (turnId: string) => cancelActiveTurn(sessionId, turnId),
      onSuccess: async () => {
        await queryClient.invalidateQueries({ queryKey: chatKeys.sessions(connectionKey) });
      },
    }),
    compact: useMutation({
      mutationFn: () => compactSession(sessionId),
      onSuccess: applySessionResult,
    }),
    clear: useMutation({
      mutationFn: () => clearSession(sessionId),
      onSuccess: applySessionResult,
    }),
  };
}
