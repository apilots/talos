import { createElectronAppServerClient, type DesktopRendererBridge } from "@talos/sdk-app-server/desktop";
import { TalosStreamTerminatedError } from "@talos/sdk-core";
import type {
  AppProtocol,
  ConfigResponse,
  ConfigPath,
  ConfigValue,
  DocUpdateRequest,
  LlmConfig,
  MemoryFileListRequest,
  MemoryFileReadRequest,
  MemoryFileWriteRequest,
  MemoryGetRequest,
  MemorySearchRequest,
  StartTurnResponse,
  SessionListRequest,
  SessionPatchRequest,
  SessionEventStreamOptions,
  TimelineEvent,
  RecoverTimelineResponse,
  WorkspaceDocKind,
} from "@talos/sdk-core";

import type { TalosWebClient } from "./client";

interface DesktopWindow {
  talos?: DesktopRendererBridge;
}

export function getDesktopBridge(): DesktopRendererBridge | undefined {
  if (typeof window === "undefined") {
    return undefined;
  }
  return (window as DesktopWindow).talos;
}

export function isDesktopRuntime(): boolean {
  return getDesktopBridge() !== undefined;
}

export function createDesktopTalosClient(
  bridge: DesktopRendererBridge,
  getToken: () => string | null | undefined = () => undefined,
): TalosWebClient {
  const client = createElectronAppServerClient(bridge, {
    getActorCredentials: () => actorCredentialsFromToken(getToken()),
  });

  const turns = {
    start: async (request: AppProtocol.StartTurnRequest): Promise<StartTurnResponse> => {
      return client.startTurn(
        toProtocolStartTurnRequest(request, request.clientRequestId ?? createClientRequestId()),
      );
    },
    retry: (request: AppProtocol.RetryTurnRequest) =>
      client.retryTurn(request),
    input: (request: AppProtocol.SubmitTurnInputRequest) => client.submitTurnInput(request),
    events: (
      sessionId: string,
      afterEventId: string | undefined,
      onEvent: (event: TimelineEvent) => void,
      signal?: AbortSignal,
      options?: SessionEventStreamOptions,
    ) => streamTimelineEvents(client, sessionId, afterEventId ?? null, onEvent, signal, options),
  };

  return {
    auth: {
      logout: () => client.authLogout().then(() => undefined),
    },
    user: {
      current: () => client.getCurrentUser(),
    },
    profile: {
      read: () => client.readProfile(),
    },
    llm: {
      get: (): Promise<ConfigResponse> => client.getLlmConfig(),
      update: (config: LlmConfig): Promise<ConfigResponse> => client.updateLlmConfig(config),
    },
    config: {
      get: (path: ConfigPath = "llm"): Promise<ConfigResponse> => client.configGet(path),
      update: (value: ConfigValue): Promise<ConfigResponse> => client.configUpdate(value),
    },
    models: {
      list: () => client.listModels(),
    },
    sessions: {
      list: (query?: SessionListRequest) =>
        client.listSessions({
          projectId: query?.projectId ?? null,
          limit: query?.limit ?? null,
          offset: query?.offset ?? null,
        }),
      get: (request: Parameters<TalosWebClient["sessions"]["get"]>[0]) =>
        client.getSession(request),
      patch: (request: SessionPatchRequest) =>
        client.patchSession({
          sessionId: request.sessionId,
          conf: toProtocolSessionConf(request.conf),
          currentDirectory: toProtocolDirectoryPatch(request.currentDirectory),
        }),
      close: (request: Parameters<TalosWebClient["sessions"]["close"]>[0]) =>
        client.closeSession(request),
      delete: (request: Parameters<TalosWebClient["sessions"]["delete"]>[0]) =>
        client.deleteSession(request),
      cancel: (request: Parameters<TalosWebClient["sessions"]["cancel"]>[0]) =>
        client.interruptTurn(request),
      compact: (request: Parameters<TalosWebClient["sessions"]["compact"]>[0]) =>
        client.compactSession(request),
      clear: (request: Parameters<TalosWebClient["sessions"]["clear"]>[0]) =>
        client.clearSession(request),
      activeTurn: (request: Parameters<TalosWebClient["sessions"]["activeTurn"]>[0]) =>
        client.getActiveTurn(request),
      history: (
        request: Parameters<TalosWebClient["sessions"]["history"]>[0],
      ) =>
        client.getSessionHistory(request),
      recoverTimeline: (
        request: Parameters<TalosWebClient["sessions"]["recoverTimeline"]>[0],
      ): Promise<RecoverTimelineResponse> =>
        client.recoverTimeline({
          sessionId: request.sessionId,
          afterEventId: request.afterEventId ?? null,
          replayLimit: request.replayLimit ?? null,
        }),
    },
    messageQueue: client.messageQueue,
    projects: {
      list: () => client.listProjects({}),
      create: (body: Parameters<TalosWebClient["projects"]["create"]>[0]) =>
        client.createProject(body),
      patch: (request: Parameters<TalosWebClient["projects"]["patch"]>[0]) =>
        client.patchProject(request),
      delete: (request: Parameters<TalosWebClient["projects"]["delete"]>[0]) =>
        client.deleteProject(request),
    },
    files: client.files,
    fs: {
      directory: {
        read: (query?: { path?: string | null }) =>
          client.readFsDirectory({ path: query?.path ?? null }),
        create: (body: { parent: string; name: string }) =>
          client.createFsDirectory(body),
      },
    },
    memory: {
      list: (query?: MemoryFileListRequest) => client.listMemoryFiles(query ?? {}),
      get: (request: MemoryFileReadRequest) => client.readMemoryFile(request),
      put: (request: MemoryFileWriteRequest) =>
        client.writeMemoryFile(request).then((response) => response),
      search: (query: MemorySearchRequest) => client.searchMemory(query),
      getEntry: (request: MemoryGetRequest) => client.getMemory(request),
    },
    workspaceDocs: {
      info: () => client.workspaceInfo(),
      get: (kind: WorkspaceDocKind) => client.readWorkspaceDoc(kind),
      put: (kind: WorkspaceDocKind, request: DocUpdateRequest) =>
        client
          .writeWorkspaceDoc(kind, request.content, expectedVersionToken(request))
          .then((response) => response),
    },
    skills: {
      list: (query?: Parameters<TalosWebClient["skills"]["list"]>[0]) => client.listSkills(query),
      suggest: (query: Parameters<TalosWebClient["skills"]["suggest"]>[0]) =>
        client.suggestSkills(query),
      read: (request: Parameters<TalosWebClient["skills"]["read"]>[0]) =>
        client.readSkill(request),
    },
    mcp: {
      import: (request: Parameters<TalosWebClient["mcp"]["import"]>[0]) => client.importMcpServer(request),
      startAuthentication: (request: Parameters<TalosWebClient["mcp"]["startAuthentication"]>[0]) => client.startMcpAuthentication(request),
      readAuthentication: (request: Parameters<TalosWebClient["mcp"]["readAuthentication"]>[0]) => client.readMcpAuthentication(request),
      updateCredentials: (request: Parameters<TalosWebClient["mcp"]["updateCredentials"]>[0]) => client.updateMcpCredentials(request),

      list: (request?: Parameters<TalosWebClient["mcp"]["list"]>[0]) => client.listMcpServers(request),
    },
    plugin: {
      search: (request: Parameters<TalosWebClient["plugin"]["search"]>[0]) =>
        client.searchPlugins(request),
      list: (request: Parameters<TalosWebClient["plugin"]["list"]>[0]) =>
        client.listPlugins(request),
      read: (request: Parameters<TalosWebClient["plugin"]["read"]>[0]) =>
        client.readPlugin(request),
      install: (request: Parameters<TalosWebClient["plugin"]["install"]>[0]) =>
        client.installPlugin(request),
      update: (request: Parameters<TalosWebClient["plugin"]["update"]>[0]) =>
        client.updatePlugin(request),
      uninstall: (request: Parameters<TalosWebClient["plugin"]["uninstall"]>[0]) =>
        client.uninstallPlugin(request),
      enable: (request: Parameters<TalosWebClient["plugin"]["enable"]>[0]) =>
        client.enablePlugin(request),
      disable: (request: Parameters<TalosWebClient["plugin"]["disable"]>[0]) =>
        client.disablePlugin(request),
      sources: {
        add: (request: Parameters<TalosWebClient["plugin"]["sources"]["add"]>[0]) =>
          client.addPluginSource(request),
      },
    },
    turns,
  } satisfies TalosWebClient;
}

function streamTimelineEvents(
  client: ReturnType<typeof createElectronAppServerClient>,
  sessionId: string,
  sinceEventId: string | null,
  onEvent: (event: AppProtocol.TimelineEvent) => void,
  signal?: AbortSignal,
  options?: SessionEventStreamOptions,
): Promise<void> {
  return streamUntilTerminal(client, sessionId, signal, (event) => {
    onEvent(event);
  }, options?.onOpen, sinceEventId, null, !options?.persistent, !options?.persistent,
  options?.onMessageQueueChanged);
}

function streamUntilTerminal(
  client: ReturnType<typeof createElectronAppServerClient>,
  sessionId: string,
  signal: AbortSignal | undefined,
  onEvent: (
    event: AppProtocol.TimelineEvent,
    stop: () => void,
  ) => void | Promise<void>,
  afterSubscribe?: () => void | Promise<void>,
  sinceEventId: string | null = null,
  turnId: string | null = null,
  closeWhenIdle = false,
  closeOnTerminal = false,
  onMessageQueueChanged?: (notification: AppProtocol.MessageQueueChangedNotification) => void,
): Promise<void> {
  return new Promise((resolve, reject) => {
    let settled = false;
    let subscriptionId: string | null = null;
    const cleanupFns: Array<() => void> = [];

    const finish = () => {
      if (settled) {
        return;
      }
      settled = true;
      for (const cleanup of cleanupFns.splice(0)) {
        cleanup();
      }
      resolve();
    };

    const fail = (error: unknown) => {
      if (settled) {
        return;
      }
      settled = true;
      for (const cleanup of cleanupFns.splice(0)) {
        cleanup();
      }
      reject(error);
    };

    const cleanupSubscription = () => {
      if (!subscriptionId) {
        return;
      }
      const id = subscriptionId;
      subscriptionId = null;
      void client.unsubscribeEvents(id).catch(() => undefined);
    };
    cleanupFns.push(cleanupSubscription);

    if (signal?.aborted) {
      finish();
      return;
    }

    const abortHandler = () => finish();
    signal?.addEventListener("abort", abortHandler, { once: true });
    if (signal) {
      cleanupFns.push(() => signal.removeEventListener("abort", abortHandler));
    }

    const unsubscribe = client.onEvent((event) => {
      if (event.event.data.identity.sessionId !== sessionId) {
        return;
      }
      void Promise.resolve().then(() => onEvent(event, finish)).catch(fail);
    });
    cleanupFns.push(unsubscribe);

    if (onMessageQueueChanged) {
      const unsubscribeQueueChanges = client.onMessageQueueChanged((notification) => {
        if (notification.sessionId === sessionId) {
          onMessageQueueChanged(notification);
        }
      });
      cleanupFns.push(unsubscribeQueueChanges);
    }

    const unsubscribeTerminated = client.onSubscriptionTerminated((notification) => {
      const matchesSubscription = subscriptionId
        ? notification.subscriptionId === subscriptionId
        : notification.sessionId === sessionId;
      if (!matchesSubscription) {
        return;
      }
      if (notification.reason === "completed") {
        finish();
        return;
      }
      fail(new TalosStreamTerminatedError(notification));
    });
    cleanupFns.push(unsubscribeTerminated);

    void client.subscribeEvents({
      sessionId,
      turnId,
      sinceEventId,
      replayLimit: null,
      closeWhenIdle,
      closeOnTerminal,
    }).then((response) => {
      if (settled) {
        void client.unsubscribeEvents(response.subscriptionId).catch(() => undefined);
        return undefined;
      }
      subscriptionId = response.subscriptionId;
      return afterSubscribe?.();
    }).catch(fail);
  });
}

function toProtocolDirectoryPatch(
  patch: SessionPatchRequest["currentDirectory"] | undefined | null,
): AppProtocol.OptionalCurrentDirectoryPatch | null {
  if (!patch) {
    return null;
  }
  return patch.type === "set"
    ? { type: "set", path: patch.path }
    : { type: "clear" };
}

function toProtocolSessionConf(
  conf: SessionPatchRequest["conf"] | undefined | null,
): AppProtocol.SessionConf | null {
  if (!conf) {
    return null;
  }
  return {
    modelSelection: conf.modelSelection,
    userPermissionMode: conf.userPermissionMode,
  };
}

function toProtocolStartTurnRequest(
  request: AppProtocol.StartTurnRequest,
  clientRequestId: string,
): AppProtocol.StartTurnRequest {
  return {
    ...request,
    clientRequestId,
  };
}


function expectedVersionToken(request: DocUpdateRequest): string | null {
  return request.base_version.mode === "match" ? request.base_version.version_token : null;
}

function createClientRequestId(): string {
  if (typeof globalThis.crypto?.randomUUID === "function") {
    return `desktop_${globalThis.crypto.randomUUID()}`;
  }
  return `desktop_${Date.now()}_${Math.random().toString(36).slice(2)}`;
}

function actorCredentialsFromToken(token: string | null | undefined): AppProtocol.ActorCredentials | null {
  const trimmed = token?.trim();
  if (!trimmed) {
    return null;
  }
  return {
    credential: {
      type: "existing_session_token",
      token: trimmed,
    },
  };
}
