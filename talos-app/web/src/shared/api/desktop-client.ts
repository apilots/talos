import { createElectronAppServerClient, type DesktopRendererBridge } from "@talos/sdk-app-server/desktop";
import type {
  AppProtocol,
  DocUpdateRequest,
  LlmConfig,
  LlmConfigRead,
  MemoryFileListParams,
  MemorySearchParams,
  SessionQuery,
  SessionUpdate,
  TalosMessageAck,
  TalosMessageRequest,
  TalosMessageRetryRequest,
  TalosSessionHistorySnapshot,
  TimelineEventEnvelope,
  TimelineRecoverySnapshot,
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
    start: async (sessionId: string, request: TalosMessageRequest): Promise<TalosMessageAck> => {
      return client.startTurn({
        session_id: sessionId,
        input: request.input as AppProtocol.ProtocolInputItem[],
        workspace_id: request.workspace_id ?? null,
        context: toProtocolContext(request.context),
        model: request.model ?? null,
        project_id: request.project_id ?? null,
        current_directory: request.current_directory ?? null,
        client_request_id: createClientRequestId(),
      });
    },
    startWithIdempotencyKey: async (
      sessionId: string,
      request: TalosMessageRequest,
      idempotencyKey: string,
    ): Promise<TalosMessageAck> => {
      return client.startTurn({
        session_id: sessionId,
        input: request.input as AppProtocol.ProtocolInputItem[],
        workspace_id: request.workspace_id ?? null,
        context: toProtocolContext(request.context),
        model: request.model ?? null,
        project_id: request.project_id ?? null,
        current_directory: request.current_directory ?? null,
        client_request_id: idempotencyKey,
      });
    },
    retry: (sessionId: string, request: TalosMessageRetryRequest) =>
      client.retryTurn({
        session_id: sessionId,
        input: request.input as AppProtocol.ProtocolInputItem[],
        workspace_id: request.workspace_id ?? null,
        client_message_id: request.client_message_id,
        retry_of_message_id: request.retry_of_message_id,
      }),
    retryWithIdempotencyKey: (
      sessionId: string,
      request: TalosMessageRetryRequest,
      idempotencyKey: string,
    ) =>
      client.retryTurn({
        session_id: sessionId,
        input: request.input as AppProtocol.ProtocolInputItem[],
        workspace_id: request.workspace_id ?? null,
        client_message_id: idempotencyKey,
        retry_of_message_id: request.retry_of_message_id,
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
    }, undefined, afterEventId ?? null, true, true),
    startEvents: (
      sessionId: string,
      request: TalosMessageRequest,
      onEvent: (event: TimelineEventEnvelope) => void,
      signal?: AbortSignal,
    ) => startTurnAndStream(client, sessionId, request, onEvent, signal),
    retryEvents: (
      sessionId: string,
      request: TalosMessageRetryRequest,
      onEvent: (event: TimelineEventEnvelope) => void,
      signal?: AbortSignal,
    ) => retryTurnAndStream(client, sessionId, request, onEvent, signal),
  };

  return {
    auth: {
      logout: () => client.authLogout().then(() => undefined),
    },
    user: {
      current: () => client.getCurrentUser(),
    },
    llm: {
      get: (): Promise<LlmConfigRead> => client.getLlmConfig(),
      update: (config: LlmConfig): Promise<LlmConfigRead> => client.updateLlmConfig(config),
    },
    models: {
      list: () => client.listModels(),
    },
    sessions: {
      list: (query?: SessionQuery) =>
        client.listSessions({
          project_id: query?.project_id ?? null,
          limit: query?.limit ?? null,
          offset: query?.offset ?? null,
        }),
      get: (sessionId: string) => client.getSession(sessionId),
      patch: (sessionId: string, update: SessionUpdate) =>
        client.patchSession({
          session_id: sessionId,
          project_id: toProtocolProjectPatch(update.project_id),
          current_directory: toProtocolDirectoryPatch(update.current_directory),
          model: update.conf?.agent?.model ?? null,
        }).then(() => undefined),
      close: (sessionId: string) =>
        client.closeSession(sessionId).then(() => undefined),
      delete: (sessionId: string) => client.deleteSession(sessionId).then(() => undefined),
      cancel: (sessionId: string) => client.interruptTurn(sessionId).then(() => undefined),
      compact: (sessionId: string) => client.compactSession(sessionId),
      clear: (sessionId: string) => client.clearSession(sessionId),
      activeTurn: (sessionId: string) => client.getActiveTurn(sessionId),
      history: (sessionId: string): Promise<TalosSessionHistorySnapshot> =>
        client.getSessionHistory(sessionId),
      recover: (
        sessionId: string,
        afterEventId?: string,
        replayLimit?: number,
      ): Promise<TimelineRecoverySnapshot> =>
        client.recoverSession(sessionId, afterEventId ?? null, replayLimit ?? null),
    },
    projects: {
      list: () => client.listProjects(),
      get: (projectId: string) => client.getProject(projectId),
      create: (body: Parameters<TalosWebClient["projects"]["create"]>[0]) =>
        client.createProject(body.name, body.directory),
      patch: (
        projectId: string,
        patch: Parameters<TalosWebClient["projects"]["patch"]>[1],
      ) => client.patchProject(projectId, patch),
      delete: (projectId: string) => client.deleteProject(projectId).then(() => undefined),
    },
    memory: {
      list: (query?: MemoryFileListParams) => client.listMemoryFiles(query?.limit),
      get: (date: string) => client.readMemoryFile(date),
      put: (date: string, request: DocUpdateRequest) =>
        client.writeMemoryFile(date, request.content, expectedVersionToken(request)),
      search: (query: MemorySearchParams) => client.searchMemory(query.q, query.limit),
    },
    workspaceDocs: {
      info: () => client.getWorkspaceInfo(),
      list: () => client.listWorkspaceDocs(),
      get: (kind: WorkspaceDocKind) => client.readWorkspaceDoc(kind),
      put: (kind: WorkspaceDocKind, request: DocUpdateRequest) =>
        client.writeWorkspaceDoc(kind, request.content, expectedVersionToken(request)),
    },
    turns,
  } as unknown as TalosWebClient;
}

async function startTurnAndStream(
  client: ReturnType<typeof createElectronAppServerClient>,
  sessionId: string,
  request: TalosMessageRequest,
  onEvent: (event: TimelineEventEnvelope) => void,
  signal?: AbortSignal,
): Promise<void> {
  const clientRequestId = createClientRequestId();
  const ack = await client.startTurn({
    session_id: sessionId,
    input: request.input as AppProtocol.ProtocolInputItem[],
    workspace_id: request.workspace_id ?? null,
    context: toProtocolContext(request.context),
    model: request.model ?? null,
    project_id: request.project_id ?? null,
    current_directory: request.current_directory ?? null,
    client_request_id: clientRequestId,
  });
  if (!ack.message_id) {
    throw new Error(`Message ack did not include message_id: ${ack.turn_id}`);
  }

  await streamUntilTerminal(client, sessionId, signal, async (event, stop) => {
    const envelope = toTimelineEnvelope(event);
    if (envelope.message_id === ack.message_id) {
      onEvent(envelope);
      if (isTerminalEvent(envelope)) {
        stop();
      }
    }
  }, undefined, null, true, true);
}

async function retryTurnAndStream(
  client: ReturnType<typeof createElectronAppServerClient>,
  sessionId: string,
  request: TalosMessageRetryRequest,
  onEvent: (event: TimelineEventEnvelope) => void,
  signal?: AbortSignal,
): Promise<void> {
  const ack = await client.retryTurn({
    session_id: sessionId,
    input: request.input as AppProtocol.ProtocolInputItem[],
    workspace_id: request.workspace_id ?? null,
    client_message_id: request.client_message_id,
    retry_of_message_id: request.retry_of_message_id,
  });
  if (!ack.message_id) {
    throw new Error(`Retry ack did not include message_id: ${ack.turn_id}`);
  }

  await streamUntilTerminal(client, sessionId, signal, async (event, stop) => {
    const envelope = toTimelineEnvelope(event);
    if (envelope.message_id === ack.message_id) {
      onEvent(envelope);
      if (isTerminalEvent(envelope)) {
        stop();
      }
    }
  }, undefined, null, true, true);
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
  }, undefined, sinceEventId, true, true);
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
  patch: SessionUpdate["project_id"] | undefined | null,
): AppProtocol.OptionalStringPatch | null {
  if (!patch) {
    return null;
  }
  return patch.type === "set"
    ? { type: "set", value: patch.project_id }
    : { type: "clear" };
}

function toProtocolDirectoryPatch(
  patch: SessionUpdate["current_directory"] | undefined | null,
): AppProtocol.OptionalPathPatch | null {
  if (!patch) {
    return null;
  }
  return patch.type === "set"
    ? { type: "set", path: patch.path }
    : { type: "clear" };
}

function toProtocolContext(
  context: TalosMessageRequest["context"] | undefined | null,
): AppProtocol.ProtocolTalosContext | null {
  if (!context) {
    return null;
  }
  return {
    source_type: "desktop",
    provider: context.provider,
    channel_id: context.channel_id ?? "desktop",
    thread_id: context.thread_id ?? null,
    message_id: context.message_id,
    trace_id: context.trace_id,
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
