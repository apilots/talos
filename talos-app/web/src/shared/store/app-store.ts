import { create } from "zustand";
import { persist, createJSONStorage } from "zustand/middleware";

import type {
  ApprovalMode,
  AdminMetric,
  FileRecord,
  MemoryRecord,
  PendingApproval,
  ProjectRecord,
  RuntimeMode,
  SessionUiStatus,
  StreamSessionState,
  ThreadFilter,
} from "@/shared/types/domain";
import {
  createDefaultFiles,
  createDefaultMemory,
  createDefaultProjects,
  getProjectRecordKey,
} from "@/shared/types/domain";

interface ConnectionState {
  serverUrl: string;
  authToken: string;
  userId: string;
  runtimeMode: RuntimeMode;
  defaultApprovalMode: ApprovalMode;
}

type UiTheme = "dark" | "light";

interface AppState {
  connection: ConnectionState;
  uiTheme: UiTheme;
  sidebarOpen: boolean;
  threadFilter: ThreadFilter;
  activeProjectId: string;
  projects: ProjectRecord[];
  sessionAliases: Record<string, string>;
  sessionApprovalModes: Record<string, ApprovalMode>;
  transientStatuses: Record<string, SessionUiStatus>;
  streams: Record<string, StreamSessionState>;
  memoryItems: MemoryRecord[];
  fileItems: FileRecord[];
  adminMetrics: AdminMetric[];
  setConnection: (patch: Partial<ConnectionState>) => void;
  setUiTheme: (theme: UiTheme) => void;
  setSidebarOpen: (open: boolean) => void;
  setThreadFilter: (filter: ThreadFilter) => void;
  setActiveProjectId: (projectId: string) => void;
  addProject: (project: ProjectRecord) => void;
  mergeProjects: (projects: ProjectRecord[], preferredProjectId?: string) => void;
  renameSessionLocal: (sessionId: string, title: string) => void;
  deleteSessionAlias: (sessionId: string) => void;
  setSessionApprovalMode: (sessionId: string, mode: ApprovalMode) => void;
  setSessionStatus: (sessionId: string, status: SessionUiStatus | undefined) => void;
  resetStream: (sessionId: string) => void;
  appendStreamText: (
    sessionId: string,
    kind: "assistant" | "thinking" | "system",
    chunk: string,
  ) => void;
  appendStreamUserInput: (sessionId: string, text: string) => void;
  upsertStreamTool: (
    sessionId: string,
    callId: string,
    payload: Partial<StreamSessionState["blocks"][number]> & {
      toolName?: string;
      arguments?: unknown;
      status?: SessionUiStatus | "queued";
      prompt?: string;
    },
  ) => void;
  appendToolProgress: (
    sessionId: string,
    callId: string,
    message: string,
    kind: "status" | "stdout" | "stderr",
  ) => void;
  completeStreamTool: (
    sessionId: string,
    callId: string,
    output: string,
    status: "done" | "error",
    timingMs?: number | null,
  ) => void;
  setPendingApproval: (sessionId: string, approval?: PendingApproval) => void;
  setStreamError: (sessionId: string, error?: string) => void;
  setStreamMeta: (
    sessionId: string,
    meta: Partial<
      Pick<
        StreamSessionState,
        "isStreaming" | "activeMessageId" | "lastSequence" | "connectionState" | "expectedMessageCount"
      >
    >,
  ) => void;
  upsertMemory: (record: MemoryRecord) => void;
  removeMemory: (id: string) => void;
  upsertFile: (record: FileRecord) => void;
  removeFile: (id: string) => void;
  setAdminMetrics: (metrics: AdminMetric[]) => void;
}

const defaultProjects = createDefaultProjects();

const memoryStorage = new Map<string, string>();

function createPersistStorage(): Storage {
  if (typeof window !== "undefined" && window.localStorage) {
    return window.localStorage;
  }

  return {
    get length() {
      return memoryStorage.size;
    },
    clear() {
      memoryStorage.clear();
    },
    getItem(key) {
      return memoryStorage.get(key) ?? null;
    },
    key(index) {
      return Array.from(memoryStorage.keys())[index] ?? null;
    },
    removeItem(key) {
      memoryStorage.delete(key);
    },
    setItem(key, value) {
      memoryStorage.set(key, value);
    },
  };
}

function ensureStreamState(state: AppState, sessionId: string): StreamSessionState {
  return (
    state.streams[sessionId] ?? {
      isStreaming: false,
      connectionState: "idle",
      blocks: [],
    }
  );
}

function normalizeProjects(projects?: ProjectRecord[]) {
  const normalized = new Map<string, ProjectRecord>();

  for (const project of defaultProjects) {
    normalized.set(getProjectRecordKey(project.root), project);
  }

  for (const project of projects ?? []) {
    const isLegacyDefaultWorkspace =
      project.origin === "default" &&
      getProjectRecordKey(project.root) !== getProjectRecordKey(defaultProjects[0].root);
    const isLegacyManagedWorkspace =
      project.root.type === "managed_workspace" &&
      (project.root.name === "talos" || project.root.name.startsWith("server-default"));
    const isLegacyWorkspacePath =
      project.root.type === "server_visible_path" &&
      project.root.path === "/srv/talos/workspace";
    if (isLegacyDefaultWorkspace || isLegacyManagedWorkspace || isLegacyWorkspacePath) {
      continue;
    }

    const key = getProjectRecordKey(project.root);
    const current = normalized.get(key);
    if (!current) {
      normalized.set(key, project);
      continue;
    }

    normalized.set(key, {
      ...project,
      name: current.origin === "default" ? current.name : project.name,
      origin: current.origin ?? project.origin,
      sessionCount: project.sessionCount ?? current.sessionCount,
      lastUsedAt: Math.max(project.lastUsedAt, current.lastUsedAt),
    });
  }

  return [...normalized.values()].sort((left, right) => right.lastUsedAt - left.lastUsedAt);
}

function mergeById<T extends { id: string }>(defaults: T[], persisted?: T[]) {
  const merged = new Map<string, T>();
  defaults.forEach((item) => merged.set(item.id, item));
  persisted?.forEach((item) => merged.set(item.id, item));
  return [...merged.values()];
}

export const useAppStore = create<AppState>()(
  persist(
    (set) => ({
      connection: {
        serverUrl: "http://localhost:8080",
        authToken: "",
        userId: "web-user",
        runtimeMode: "cloud",
        defaultApprovalMode: "needs_approval",
      },
      uiTheme: "light",
      sidebarOpen: true,
      threadFilter: "all",
      activeProjectId: defaultProjects[0].id,
      projects: defaultProjects,
      sessionAliases: {},
      sessionApprovalModes: {},
      transientStatuses: {},
      streams: {},
      memoryItems: createDefaultMemory(),
      fileItems: createDefaultFiles(),
      adminMetrics: [],
      setConnection: (patch) =>
        set((state) => ({
          connection: {
            ...state.connection,
            ...patch,
          },
        })),
      setUiTheme: (uiTheme) => set({ uiTheme }),
      setSidebarOpen: (sidebarOpen) => set({ sidebarOpen }),
      setThreadFilter: (threadFilter) => set({ threadFilter }),
      setActiveProjectId: (activeProjectId) => set({ activeProjectId }),
      addProject: (project) =>
        set((state) => ({
          projects: normalizeProjects([project, ...state.projects]),
          activeProjectId: project.id,
        })),
      mergeProjects: (projects, preferredProjectId) =>
        set((state) => {
          const merged = normalizeProjects([...state.projects, ...projects]);
          const activeProjectExists = merged.some((project) => project.id === state.activeProjectId);
          const shouldFollowPreferred =
            !activeProjectExists || state.activeProjectId === defaultProjects[0].id;
          return {
            projects: merged,
            activeProjectId:
              shouldFollowPreferred && preferredProjectId
                ? preferredProjectId
                : activeProjectExists
                  ? state.activeProjectId
                  : merged[0]?.id ?? defaultProjects[0].id,
          };
        }),
      renameSessionLocal: (sessionId, title) =>
        set((state) => ({
          sessionAliases: {
            ...state.sessionAliases,
            [sessionId]: title,
          },
        })),
      deleteSessionAlias: (sessionId) =>
        set((state) => {
          const aliases = { ...state.sessionAliases };
          delete aliases[sessionId];
          return { sessionAliases: aliases };
        }),
      setSessionApprovalMode: (sessionId, mode) =>
        set((state) => ({
          sessionApprovalModes: {
            ...state.sessionApprovalModes,
            [sessionId]: mode,
          },
        })),
      setSessionStatus: (sessionId, status) =>
        set((state) => {
          const next = { ...state.transientStatuses };
          if (!status) {
            delete next[sessionId];
          } else {
            next[sessionId] = status;
          }
          return { transientStatuses: next };
        }),
      resetStream: (sessionId) =>
        set((state) => ({
          streams: {
            ...state.streams,
            [sessionId]: { isStreaming: false, connectionState: "idle", blocks: [] },
          },
        })),
      appendStreamText: (sessionId, kind, chunk) =>
        set((state) => {
          const stream = ensureStreamState(state, sessionId);
          const blocks = [...stream.blocks];
          const lastBlock = blocks.at(-1);
          if (
            lastBlock &&
            (lastBlock.kind === "assistant" ||
              lastBlock.kind === "thinking" ||
              lastBlock.kind === "system") &&
            lastBlock.kind === kind
          ) {
            lastBlock.text += chunk;
          } else {
            blocks.push({
              id: `${kind}-${Date.now()}-${blocks.length}`,
              kind,
              text: chunk,
              live: true,
            });
          }
          return {
            streams: {
              ...state.streams,
              [sessionId]: { ...stream, isStreaming: true, blocks },
            },
          };
        }),
      appendStreamUserInput: (sessionId, text) =>
        set((state) => {
          const stream = ensureStreamState(state, sessionId);
          const blocks = [
            ...stream.blocks,
            {
              id: `user-${Date.now()}-${stream.blocks.length}`,
              kind: "user" as const,
              text,
              live: true,
            },
          ];
          return {
            streams: {
              ...state.streams,
              [sessionId]: { ...stream, isStreaming: true, blocks },
            },
          };
        }),
      upsertStreamTool: (sessionId, callId, payload) =>
        set((state) => {
          const stream = ensureStreamState(state, sessionId);
          const blocks = [...stream.blocks];
          const index = blocks.findIndex(
            (block) => block.kind === "tool" && block.callId === callId,
          );
          if (index >= 0) {
            const current = blocks[index];
            if (current.kind === "tool") {
              blocks[index] = {
                ...current,
                toolName: payload.toolName ?? current.toolName,
                arguments: payload.arguments ?? current.arguments,
                status: (payload.status as typeof current.status | undefined) ?? current.status,
                prompt: payload.prompt ?? current.prompt,
                live: true,
              };
            }
          } else {
            blocks.push({
              id: `tool-${callId}`,
              kind: "tool",
              callId,
              toolName: payload.toolName ?? "tool",
              arguments: payload.arguments ?? null,
              status: (payload.status as "queued" | "running" | "waiting_approval" | "done" | "error" | undefined) ?? "queued",
              progress: [],
              prompt: payload.prompt,
              live: true,
            });
          }
          return {
            streams: {
              ...state.streams,
              [sessionId]: { ...stream, isStreaming: true, blocks },
            },
          };
        }),
      appendToolProgress: (sessionId, callId, message, kind) =>
        set((state) => {
          const stream = ensureStreamState(state, sessionId);
          const blocks = stream.blocks.map((block) => {
            if (block.kind !== "tool" || block.callId !== callId) {
              return block;
            }
            return {
              ...block,
              progress: [...block.progress, { kind, message }],
              live: true,
            };
          });
          return {
            streams: {
              ...state.streams,
              [sessionId]: { ...stream, blocks },
            },
          };
        }),
      completeStreamTool: (sessionId, callId, output, status, timingMs) =>
        set((state) => {
          const stream = ensureStreamState(state, sessionId);
          const blocks = stream.blocks.map((block) => {
            if (block.kind !== "tool" || block.callId !== callId) {
              return block;
            }
            return {
              ...block,
              output,
              status,
              timingMs,
              live: true,
            };
          });
          return {
            streams: {
              ...state.streams,
              [sessionId]: { ...stream, blocks },
            },
          };
        }),
      setPendingApproval: (sessionId, pendingApproval) =>
        set((state) => {
          const stream = ensureStreamState(state, sessionId);
          return {
            streams: {
              ...state.streams,
              [sessionId]: { ...stream, pendingApproval },
            },
          };
        }),
      setStreamError: (sessionId, error) =>
        set((state) => {
          const stream = ensureStreamState(state, sessionId);
          return {
            streams: {
              ...state.streams,
              [sessionId]: error
                ? { ...stream, error, isStreaming: false, connectionState: "idle" }
                : { ...stream, error: undefined },
            },
          };
        }),
      setStreamMeta: (sessionId, meta) =>
        set((state) => {
          const stream = ensureStreamState(state, sessionId);
          return {
            streams: {
              ...state.streams,
              [sessionId]: { ...stream, ...meta },
            },
          };
        }),
      upsertMemory: (record) =>
        set((state) => ({
          memoryItems: [record, ...state.memoryItems.filter((item) => item.id !== record.id)],
        })),
      removeMemory: (id) =>
        set((state) => ({
          memoryItems: state.memoryItems.filter((item) => item.id !== id),
        })),
      upsertFile: (record) =>
        set((state) => ({
          fileItems: [record, ...state.fileItems.filter((item) => item.id !== record.id)],
        })),
      removeFile: (id) =>
        set((state) => ({
          fileItems: state.fileItems.filter((item) => item.id !== id),
        })),
      setAdminMetrics: (adminMetrics) => set({ adminMetrics }),
    }),
    {
      name: "talos-web-ui",
      storage: createJSONStorage(createPersistStorage),
      merge: (persistedState, currentState) => {
        const typedState = persistedState as Partial<AppState> | undefined;
        const projects = normalizeProjects(typedState?.projects);
        const activeProjectExists = projects.some(
          (project) => project.id === typedState?.activeProjectId,
        );
        return {
          ...currentState,
          ...typedState,
          uiTheme: typedState?.uiTheme ?? currentState.uiTheme,
          threadFilter: "all",
          projects,
          activeProjectId: activeProjectExists ? typedState?.activeProjectId ?? projects[0].id : projects[0].id,
          sessionApprovalModes: typedState?.sessionApprovalModes ?? {},
          transientStatuses: typedState?.transientStatuses ?? {},
          streams: typedState?.streams ?? {},
          memoryItems: mergeById(currentState.memoryItems, typedState?.memoryItems),
          fileItems: mergeById(currentState.fileItems, typedState?.fileItems),
          connection: {
            ...currentState.connection,
            ...typedState?.connection,
            defaultApprovalMode: typedState?.connection?.defaultApprovalMode ?? currentState.connection.defaultApprovalMode,
          },
        };
      },
      partialize: (state) => ({
        connection: state.connection,
        uiTheme: state.uiTheme,
        sidebarOpen: state.sidebarOpen,
        activeProjectId: state.activeProjectId,
        projects: state.projects,
        sessionAliases: state.sessionAliases,
        sessionApprovalModes: state.sessionApprovalModes,
        transientStatuses: state.transientStatuses,
        streams: state.streams,
        memoryItems: state.memoryItems,
        fileItems: state.fileItems,
      }),
    },
  ),
);

export function getConnectionSettings() {
  return useAppStore.getState().connection;
}
