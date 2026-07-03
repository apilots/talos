import { createElectronAppServerClient, type DesktopRendererBridge } from "@talos/sdk-app-server/desktop";
import type {
  AppProtocol,
  ConfigGetResponse,
  ConfigUpdateResponse,
  DocUpdateRequest,
  LlmConfig,
  MemoryFileListRequest,
  MemoryFileReadRequest,
  MemoryFileWriteRequest,
  MemorySearchRequest,
  RetryTurnRequest,
  StartTurnRequest,
  StartTurnResponse,
  SessionListRequest,
  SessionPatchRequest,
  TimelineEventEnvelope,
  TimelineRecoveryResponse,
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
        toProtocolStartTurnRequest(request, request.client_request_id ?? createClientRequestId()),
      );
    },
    startWithIdempotencyKey: async (
      request: AppProtocol.StartTurnRequest,
      idempotencyKey: string,
    ): Promise<StartTurnResponse> => {
      return client.startTurn(toProtocolStartTurnRequest(request, idempotencyKey));
    },
    retry: (request: AppProtocol.RetryTurnRequest) =>
      client.retryTurn(request),
    retryWithIdempotencyKey: (
      request: AppProtocol.RetryTurnRequest,
      idempotencyKey: string,
    ) =>
      client.retryTurn({
        ...request,
        client_message_id: idempotencyKey,
      }),
    events: (
      sessionId: string,
      afterEventId: string | undefined,
      onEvent: (event: TimelineEventEnvelope) => void,
      signal?: AbortSignal,
    ) => streamTimelineEvents(client, sessionId, afterEventId ?? null, (event) => {
      onEvent(toTimelineEnvelope(event));
    }, signal),
    eventsForMessage: (
      sessionId: string,
      messageId: string,
      onEvent: (event: TimelineEventEnvelope) => void,
      signal?: AbortSignal,
      afterEventId?: string,
    ) => streamUntilTerminal(client, sessionId, signal, (event, stop) => {
      const envelope = toTimelineEnvelope(event);
      if (envelope.message_id !== messageId) {
        return;
      }
      onEvent(envelope);
      if (isTerminalEvent(envelope)) {
        stop();
      }
    }, undefined, afterEventId ?? null, null, true, true),
    startEvents: (
      request: StartTurnRequest,
      onEvent: (event: TimelineEventEnvelope) => void,
      signal?: AbortSignal,
    ) => startTurnAndStream(client, request, onEvent, signal),
    retryEvents: (
      request: RetryTurnRequest,
      onEvent: (event: TimelineEventEnvelope) => void,
      signal?: AbortSignal,
    ) => retryTurnAndStream(client, request, onEvent, signal),
  };

  return {
    auth: {
      logout: () => client.authLogout().then(() => undefined),
    },
    user: {
      current: () => client.getCurrentUser(),
    },
    llm: {
      get: (): Promise<ConfigGetResponse> => client.getLlmConfig(),
      update: (config: LlmConfig): Promise<ConfigUpdateResponse> => client.updateLlmConfig(config),
    },
    models: {
      list: () => client.listModels(),
    },
    sessions: {
      list: (query?: SessionListRequest) =>
        client.listSessions({
          project_id: query?.project_id ?? null,
          limit: query?.limit ?? null,
          offset: query?.offset ?? null,
        }),
      get: (request: Parameters<TalosWebClient["sessions"]["get"]>[0]) =>
        client.getSession(request),
      patch: (request: SessionPatchRequest) =>
        client.patchSession({
          session_id: request.session_id,
          conf: toProtocolSessionConf(request.conf),
          project_id: toProtocolProjectPatch(request.project_id),
          current_directory: toProtocolDirectoryPatch(request.current_directory),
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
      ): Promise<TimelineRecoveryResponse> =>
        client.recoverTimeline({
          session_id: request.session_id,
          after_event_id: request.after_event_id ?? null,
          replay_limit: request.replay_limit ?? null,
        }),
      recoverTimelineById: (
        sessionId: string,
        afterEventId?: string,
        replayLimit?: number,
      ): Promise<TimelineRecoveryResponse> =>
        client.recoverTimeline({
          session_id: sessionId,
          after_event_id: afterEventId ?? null,
          replay_limit: replayLimit ?? null,
        }),
    },
    projects: {
      list: () => client.listProjects({}),
      get: (request: Parameters<TalosWebClient["projects"]["get"]>[0]) =>
        client.getProject(request),
      create: (body: Parameters<TalosWebClient["projects"]["create"]>[0]) =>
        client.createProject(body),
      patch: (request: Parameters<TalosWebClient["projects"]["patch"]>[0]) =>
        client.patchProject(request),
      delete: (request: Parameters<TalosWebClient["projects"]["delete"]>[0]) =>
        client.deleteProject(request),
    },
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
    },
    workspaceDocs: {
      info: () => client.getWorkspaceInfo(),
      list: () => client.listWorkspaceDocs(),
      get: (kind: WorkspaceDocKind) => client.readWorkspaceDoc(kind),
      put: (kind: WorkspaceDocKind, request: DocUpdateRequest) =>
        client
          .writeWorkspaceDoc(kind, request.content, expectedVersionToken(request))
          .then((response) => response),
    },
    skills: {
      list: (query?: Parameters<TalosWebClient["skills"]["list"]>[0]) => client.listSkills(query),
      refresh: (request?: Parameters<TalosWebClient["skills"]["refresh"]>[0]) =>
        client.refreshSkills(request),
      suggest: (query: Parameters<TalosWebClient["skills"]["suggest"]>[0]) =>
        client.suggestSkills(query),
      read: (request: Parameters<TalosWebClient["skills"]["read"]>[0]) =>
        client.readSkill(request),
      resolve: (request: Parameters<TalosWebClient["skills"]["resolve"]>[0]) =>
        client.resolveSkills(request),
    },
    plugin: {
      search: (request: Parameters<TalosWebClient["plugin"]["search"]>[0]) =>
        client.searchPlugins(request),
      list: (request: Parameters<TalosWebClient["plugin"]["list"]>[0]) =>
        client.listPlugins(request),
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
        list: (query?: Parameters<TalosWebClient["plugin"]["sources"]["list"]>[0]) =>
          client.listPluginSources(query),
        add: (request: Parameters<TalosWebClient["plugin"]["sources"]["add"]>[0]) =>
          client.addPluginSource(request),
        refresh: (request: Parameters<TalosWebClient["plugin"]["sources"]["refresh"]>[0]) =>
          client.refreshPluginSource(request),
        remove: (request: Parameters<TalosWebClient["plugin"]["sources"]["remove"]>[0]) =>
          client.removePluginSource(request),
      },
    },
    turns,
  } as unknown as TalosWebClient;
}

async function startTurnAndStream(
  client: ReturnType<typeof createElectronAppServerClient>,
  request: StartTurnRequest,
  onEvent: (event: TimelineEventEnvelope) => void,
  signal?: AbortSignal,
): Promise<void> {
  const clientRequestId = createClientRequestId();
  const protocolRequest = toProtocolStartTurnRequest(
    request as AppProtocol.StartTurnRequest,
    request.client_request_id ?? clientRequestId,
  );
  const ack = await client.startTurn(protocolRequest);
  if (!ack.message_id) {
    throw new Error(`Message ack did not include message_id: ${ack.turn_id}`);
  }

  await streamUntilTerminal(client, request.session_id, signal, async (event, stop) => {
    const envelope = toTimelineEnvelope(event);
    if (envelope.message_id === ack.message_id) {
      onEvent(envelope);
      if (isTerminalEvent(envelope)) {
        stop();
      }
    }
  }, undefined, null, ack.turn_id, true, true);
}

async function retryTurnAndStream(
  client: ReturnType<typeof createElectronAppServerClient>,
  request: RetryTurnRequest,
  onEvent: (event: TimelineEventEnvelope) => void,
  signal?: AbortSignal,
): Promise<void> {
  const ack = await client.retryTurn(request as AppProtocol.RetryTurnRequest);
  if (!ack.message_id) {
    throw new Error(`Retry ack did not include message_id: ${ack.turn_id}`);
  }

  await streamUntilTerminal(client, request.session_id, signal, async (event, stop) => {
    const envelope = toTimelineEnvelope(event);
    if (envelope.message_id === ack.message_id) {
      onEvent(envelope);
      if (isTerminalEvent(envelope)) {
        stop();
      }
    }
  }, undefined, null, ack.turn_id, true, true);
}

function streamTimelineEvents(
  client: ReturnType<typeof createElectronAppServerClient>,
  sessionId: string,
  sinceEventId: string | null,
  onEvent: (event: AppProtocol.TimelineEventEnvelope) => void,
  signal?: AbortSignal,
): Promise<void> {
  return streamUntilTerminal(client, sessionId, signal, (event) => {
    onEvent(event);
  }, undefined, sinceEventId, null, true, true);
}

function streamUntilTerminal(
  client: ReturnType<typeof createElectronAppServerClient>,
  sessionId: string,
  signal: AbortSignal | undefined,
  onEvent: (
    event: AppProtocol.TimelineEventEnvelope,
    stop: () => void,
  ) => void | Promise<void>,
  afterSubscribe?: () => void | Promise<void>,
  sinceEventId: string | null = null,
  turnId: string | null = null,
  closeWhenIdle = false,
  closeOnTerminal = false,
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
      if (event.session_id !== sessionId) {
        return;
      }
      void Promise.resolve(onEvent(event, finish)).catch(fail);
    });
    cleanupFns.push(unsubscribe);

    const unsubscribeTerminated = client.onSubscriptionTerminated((envelope) => {
      const matchesSubscription = subscriptionId
        ? envelope.subscription_id === subscriptionId
        : envelope.session_id === sessionId;
      if (!matchesSubscription) {
        return;
      }
      if (envelope.reason === "completed") {
        finish();
        return;
      }
      fail(new Error(`Desktop event subscription terminated: ${envelope.reason}: ${envelope.message}`));
    });
    cleanupFns.push(unsubscribeTerminated);

    void client.subscribeEvents({
      session_id: sessionId,
      turn_id: turnId,
      since_event_id: sinceEventId,
      replay_limit: null,
      close_when_idle: closeWhenIdle,
      close_on_terminal: closeOnTerminal,
    }).then((response) => {
      if (settled) {
        void client.unsubscribeEvents(response.subscription_id).catch(() => undefined);
        return undefined;
      }
      subscriptionId = response.subscription_id;
      return afterSubscribe?.();
    }).catch(fail);
  });
}

function toTimelineEnvelope(event: AppProtocol.TimelineEventEnvelope): TimelineEventEnvelope {
  return {
    ...event,
    event_seq: Number(event.event_seq),
    created_at_ms: Number(event.created_at_ms),
    event: {
      ...event.event,
      event_seq: event.event.event_seq == null ? event.event.event_seq : Number(event.event.event_seq),
      created_at_ms: event.event.created_at_ms == null ? event.event.created_at_ms : Number(event.event.created_at_ms),
      sequence: Number(event.event.sequence),
    },
  } as TimelineEventEnvelope;
}

function isTerminalEvent(event: TimelineEventEnvelope): boolean {
  return event.event.event_type === "turn_completed" || event.event.event_type === "turn_error";
}

function toProtocolProjectPatch(
  patch: SessionPatchRequest["project_id"] | undefined | null,
): AppProtocol.OptionalProjectIdPatch | null {
  if (!patch) {
    return null;
  }
  return patch.type === "set"
    ? { type: "set", project_id: patch.project_id }
    : { type: "clear" };
}

function toProtocolDirectoryPatch(
  patch: SessionPatchRequest["current_directory"] | undefined | null,
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
    model_ref: conf.model_ref ?? null,
  };
}

function toProtocolStartTurnRequest(
  request: AppProtocol.StartTurnRequest,
  clientRequestId: string,
): AppProtocol.StartTurnRequest {
  return {
    ...request,
    context: request.context
      ? ({
          ...request.context,
          source_type: "desktop",
          channel_id: request.context.channel_id ?? "desktop",
          thread_id: request.context.thread_id ?? null,
          project_root: null,
          environment: request.context.environment ?? null,
        } as AppProtocol.TalosContext)
      : undefined,
    client_request_id: clientRequestId,
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
