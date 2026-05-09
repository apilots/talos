import { createElectronAppServerClient, type DesktopRendererBridge } from "@talos/sdk-app-server/desktop";
import type {
  AdminCreateUserRequest,
  AdminPatchUserRequest,
  AppProtocol,
  CreateUserTokenRequest,
  DocUpdateRequest,
  LlmConfig,
  LlmConfigRead,
  LoginRequest,
  MemoryFileListParams,
  MemorySearchParams,
  PaginatedResponse,
  RegisterRequest,
  SessionQuery,
  SessionUpdate,
  TalosEvent,
  TalosMessageAck,
  TalosMessageRequest,
  TalosMessageRetryRequest,
  TalosSession,
  TalosSessionEventRecord,
  ToolApprovalRequest,
  UpdateCurrentUserRequest,
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

  const messages = {
    send: async (sessionId: string, request: TalosMessageRequest): Promise<TalosMessageAck> => {
      return client.startTurn({
        session_id: sessionId,
        input: request.input as AppProtocol.ProtocolInputItem[],
        workspace_id: request.workspace_id ?? null,
        client_request_id: createClientRequestId(),
      });
    },
    sendWithIdempotencyKey: async (
      sessionId: string,
      request: TalosMessageRequest,
      idempotencyKey: string,
    ): Promise<TalosMessageAck> => {
      return client.startTurn({
        session_id: sessionId,
        input: request.input as AppProtocol.ProtocolInputItem[],
        workspace_id: request.workspace_id ?? null,
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
      onEvent: (record: TalosSessionEventRecord) => void,
      signal?: AbortSignal,
    ) => streamSessionEvents(client, sessionId, afterEventId ?? null, (envelope) => {
      onEvent(toSessionEventRecord(envelope));
    }, signal),
    eventsForMessage: (
      sessionId: string,
      messageId: string,
      onEvent: (event: TalosEvent) => void,
      signal?: AbortSignal,
    ) => streamUntilTerminal(client, sessionId, signal, (envelope, stop) => {
      const event = toTalosEvent(envelope.event);
      if (event.message_id !== messageId) {
        return;
      }
      onEvent(event);
      if (isTerminalEvent(event)) {
        stop();
      }
    }),
    sendEvents: (
      sessionId: string,
      request: TalosMessageRequest,
      onEvent: (event: TalosEvent) => void,
      signal?: AbortSignal,
    ) => startTurnAndStream(client, sessionId, request, onEvent, signal),
    retryEvents: (
      sessionId: string,
      request: TalosMessageRetryRequest,
      onEvent: (event: TalosEvent) => void,
      signal?: AbortSignal,
    ) => retryTurnAndStream(client, sessionId, request, onEvent, signal),
  };

  return {
    auth: {
      register: (request: RegisterRequest) => client.authRegister(request),
      login: (request: LoginRequest) => client.authLogin(request),
      logout: () => client.authLogout().then(() => undefined),
    },
    user: {
      current: () => client.getCurrentUser(),
      update: (request: UpdateCurrentUserRequest) => client.updateCurrentUser(request),
      tokens: {
        list: () => client.listUserTokens(),
        create: (request: CreateUserTokenRequest) => client.createUserToken(request),
        delete: (tokenId: string) => client.deleteUserToken(tokenId).then(() => undefined),
      },
    },
    admin: {
      users: {
        list: () => client.listAdminUsers(),
        create: (request: AdminCreateUserRequest) => client.createAdminUser(request),
        patch: (userId: string, request: AdminPatchUserRequest) =>
          client.patchAdminUser(userId, request),
        delete: (userId: string) => client.deleteAdminUser(userId).then(() => undefined),
      },
      sessions: {
        list: (query?: SessionQuery): Promise<PaginatedResponse<TalosSession>> =>
          client.listAdminSessions({
            user_id: query?.user_id ?? null,
            channel_id: query?.channel_id ?? null,
            provider: query?.provider ?? null,
            project_id: query?.project_id ?? null,
            limit: query?.limit ?? null,
            offset: query?.offset ?? null,
          }),
        delete: (sessionId: string) => client.deleteSession(sessionId).then(() => undefined),
      },
      projects: {
        list: () => client.listAdminProjects(),
      },
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
      create: (body: Parameters<TalosWebClient["sessions"]["create"]>[0]) =>
        client.createSession({
          context: {
            source_type: "desktop",
            provider: body.context.provider,
            channel_id: body.context.channel_id ?? "desktop",
            thread_id: body.context.thread_id ?? null,
            message_id: body.context.message_id,
            trace_id: body.context.trace_id,
          },
          model: body.model ?? body.conf?.agent?.model ?? null,
          project_id: body.project_id ?? null,
          current_directory: body.current_directory ?? null,
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
      turns: (sessionId: string) => client.getSessionTurns(sessionId),
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
      list: () => client.listWorkspaceDocs(),
      get: (kind: WorkspaceDocKind) => client.readWorkspaceDoc(kind),
      put: (kind: WorkspaceDocKind, request: DocUpdateRequest) =>
        client.writeWorkspaceDoc(kind, request.content, expectedVersionToken(request)),
    },
    messages,
    toolApprovals: {
      submit: (sessionId: string, request: ToolApprovalRequest) =>
        client.submitToolApproval({
          session_id: sessionId,
          call_id: request.call_id,
          tool_name: request.tool_name,
          approved: request.approved,
          reason: request.reason ?? null,
        }).then(() => undefined),
    },
  } as unknown as TalosWebClient;
}

async function startTurnAndStream(
  client: ReturnType<typeof createElectronAppServerClient>,
  sessionId: string,
  request: TalosMessageRequest,
  onEvent: (event: TalosEvent) => void,
  signal?: AbortSignal,
): Promise<void> {
  const clientRequestId = createClientRequestId();
  const ack = await client.startTurn({
    session_id: sessionId,
    input: request.input as AppProtocol.ProtocolInputItem[],
    workspace_id: request.workspace_id ?? null,
    client_request_id: clientRequestId,
  });
  if (!ack.message_id) {
    throw new Error(`Message ack did not include message_id: ${ack.turn_id}`);
  }

  await streamUntilTerminal(client, sessionId, signal, async (envelope, stop) => {
    if (envelope.event.message_id === ack.message_id) {
      const event = toTalosEvent(envelope.event);
      onEvent(event);
      if (isTerminalEvent(event)) {
        stop();
      }
    }
  });
}

async function retryTurnAndStream(
  client: ReturnType<typeof createElectronAppServerClient>,
  sessionId: string,
  request: TalosMessageRetryRequest,
  onEvent: (event: TalosEvent) => void,
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

  await streamUntilTerminal(client, sessionId, signal, async (envelope, stop) => {
    if (envelope.event.message_id === ack.message_id) {
      const event = toTalosEvent(envelope.event);
      onEvent(event);
      if (isTerminalEvent(event)) {
        stop();
      }
    }
  });
}

function streamSessionEvents(
  client: ReturnType<typeof createElectronAppServerClient>,
  sessionId: string,
  sinceEventId: string | null,
  onEnvelope: (envelope: AppProtocol.ApplicationEventEnvelope) => void,
  signal?: AbortSignal,
): Promise<void> {
  return streamUntilTerminal(client, sessionId, signal, (envelope) => {
    onEnvelope(envelope);
  }, undefined, sinceEventId);
}

function streamUntilTerminal(
  client: ReturnType<typeof createElectronAppServerClient>,
  sessionId: string,
  signal: AbortSignal | undefined,
  onEnvelope: (
    envelope: AppProtocol.ApplicationEventEnvelope,
    stop: () => void,
  ) => void | Promise<void>,
  afterSubscribe?: () => void | Promise<void>,
  sinceEventId: string | null = null,
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

    const unsubscribe = client.onEvent((envelope: AppProtocol.ApplicationEventEnvelope) => {
      if (envelope.event.session_id !== sessionId) {
        return;
      }
      void Promise.resolve(onEnvelope(envelope, finish)).catch(fail);
    });
    cleanupFns.push(unsubscribe);

    const unsubscribeTerminated = client.onSubscriptionTerminated((envelope) => {
      const matchesSubscription = subscriptionId
        ? envelope.subscription_id === subscriptionId
        : envelope.session_id === sessionId;
      if (!matchesSubscription) {
        return;
      }
      fail(new Error(`Desktop event subscription terminated: ${envelope.reason}: ${envelope.message}`));
    });
    cleanupFns.push(unsubscribeTerminated);

    void client.subscribeEvents({
      session_id: sessionId,
      since_event_id: sinceEventId,
      replay_limit: null,
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

function toSessionEventRecord(envelope: AppProtocol.ApplicationEventEnvelope): TalosSessionEventRecord {
  const event = toTalosEvent(envelope.event);
  return {
    event_seq: Number(event.sequence),
    event_id: envelope.event_id,
    session_id: event.session_id,
    turn_id: envelope.turn_id,
    message_id: event.message_id,
    sequence: event.sequence,
    event_type: event.event_type,
    payload: event.payload,
    created_at: Date.now(),
  };
}

function toTalosEvent(event: AppProtocol.TalosEvent): TalosEvent {
  return {
    ...event,
    sequence: Number(event.sequence),
  } as TalosEvent;
}

function isTerminalEvent(event: TalosEvent): boolean {
  return event.event_type === "message_completed" || event.event_type === "message_error";
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
