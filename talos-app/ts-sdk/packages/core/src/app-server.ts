import { decodeFileContent } from "./file-content";
import type * as AppProtocol from "./generated/app-protocol";
import type { FileUploadOptions } from "./adapters";
import type {
  MemoryFileListResponse,
  MemoryFileReadRequest,
  MemoryFileReadResponse,
  MemoryFileWriteRequest,
  MemoryFileWriteResponse,
  MemoryFileListRequest,
  MemoryGetRequest,
  MemoryGetResponse,
  MemorySearchRequest,
  MemorySearchResponse,
  FsDirectoryCreateResponse,
  FsDirectoryReadResponse,
  FileDeleteRequest,
  FileDeleteResponse,
  FileListRequest,
  FileListResponse,
  FileReadRequest,
  FileReadResponse,
  FileUpdateRequest,
  FileUpdateResponse,
  FileUploadData,
  FileUploadRequest,
  FileUploadResponse,
  ConfigDeleteResponse,
  ConfigResponse,
  ConfigReloadResponse,
  ConfigPath,
  ConfigValue,
  LlmConfig,
  ModelListResponse,
  ProfileReadResponse,
  ProjectCreateResponse,
  ProjectDeleteResponse,
  ProjectListResponse,
  ProjectPatch,
  ProjectPatchResponse,
  ProjectReadResponse,
  SessionActiveTurnResponse,
  SessionHistoryResponse,
  SessionListResponse,
  SessionUpdateResponse,
  SessionCompactResponse,
  SessionClearResponse,
  SessionDeleteResponse,
  RecoverTimelineResponse,
  UserCurrentResponse,
  AuthLogoutResponse,
  AuthSessionClearResponse,
  AuthSessionReadResponse,
  CloudLoginCancelResponse,
  SessionCloseResponse,
  WorkspaceDocKind,
  WorkspaceInfoResponse,
  WorkspaceDocListResponse,
  WorkspaceDocReadResponse,
  WorkspaceDocWriteResponse,
} from "./types";

export type AppServerEventHandler = (
  event: AppProtocol.TimelineEvent,
) => void;

export type AppServerSubscriptionTerminatedHandler = (
  notification: AppProtocol.SubscriptionTerminatedNotification,
) => void;

export type AppServerMessageQueueChangedHandler = (
  notification: AppProtocol.MessageQueueChangedNotification,
) => void;

export type AppServerActiveTurnChangedHandler = (notification: AppProtocol.SessionActiveTurnChangedNotification) => void;

export type AppServerErrorHandler = (error: Error) => void;

export interface AppServerRequestOptions {
  actorCredentials?: AppProtocol.ActorCredentials | null;
}

export interface AppServerTransport {
  request<T>(
    method: AppProtocol.ClientMethod,
    params: unknown,
    options?: AppServerRequestOptions,
  ): Promise<T>;
  onEvent(handler: AppServerEventHandler): () => void;
  onActiveTurnChanged?(handler: AppServerActiveTurnChangedHandler): () => void;
  onMessageQueueChanged?(handler: AppServerMessageQueueChangedHandler): () => void;
  onSubscriptionTerminated?(handler: AppServerSubscriptionTerminatedHandler): () => void;
  onError?(handler: AppServerErrorHandler): () => void;
}

export class AppServerClient {
  constructor(protected readonly transport: AppServerTransport) {}

  onEvent(handler: AppServerEventHandler): () => void {
    return this.transport.onEvent(handler);
  }

  onActiveTurnChanged(handler: AppServerActiveTurnChangedHandler): () => void { return this.transport.onActiveTurnChanged?.(handler) ?? (() => undefined); }

  onMessageQueueChanged(handler: AppServerMessageQueueChangedHandler): () => void {
    return this.transport.onMessageQueueChanged?.(handler) ?? (() => undefined);
  }

  onSubscriptionTerminated(handler: AppServerSubscriptionTerminatedHandler): () => void {
    return this.transport.onSubscriptionTerminated?.(handler) ?? (() => undefined);
  }

  onError(handler: AppServerErrorHandler): () => void {
    return this.transport.onError?.(handler) ?? (() => undefined);
  }

  files = {
    upload: async (
      request: FileUploadRequest,
      data: FileUploadData,
      options: FileUploadOptions = {},
    ): Promise<FileUploadResponse> => {
      throwIfUploadAborted(options.signal);
      const totalBytes = data instanceof Blob ? data.size : data.byteLength;
      options.onProgress?.({ loadedBytes: 0, totalBytes });
      const bytes = data instanceof Blob ? new Uint8Array(await data.arrayBuffer()) : data;
      throwIfUploadAborted(options.signal);
      const response = await this.transport.request<FileUploadResponse>("file/upload", {
        request,
        dataBase64: encodeBase64(bytes),
      } satisfies AppProtocol.FileUploadParams);
      if (options.signal?.aborted) {
        try {
          await this.transport.request<FileDeleteResponse>("file/delete", {
            fileId: response.file.fileId,
          } satisfies FileDeleteRequest);
        } catch (cleanupError) {
          throw new Error("Cancelled file upload cleanup failed.", { cause: cleanupError });
        }
      }
      throwIfUploadAborted(options.signal);
      options.onProgress?.({ loadedBytes: totalBytes, totalBytes });
      return response;
    },
    list: (request: FileListRequest = {}): Promise<FileListResponse> =>
      this.transport.request("file/list", request),
    read: (request: FileReadRequest): Promise<FileReadResponse> =>
      this.transport.request("file/read", request),
    content: (request: FileReadRequest): Promise<import("./types").FileDownloadResponse> =>
      this.transport.request<import("./generated/app-protocol").FileContentResponse>("file/content", request).then(decodeFileContent),
    update: (request: FileUpdateRequest): Promise<FileUpdateResponse> =>
      this.transport.request("file/update", request),
    delete: (request: FileDeleteRequest): Promise<FileDeleteResponse> =>
      this.transport.request("file/delete", request),
  };

  listModels(params: AppProtocol.ModelListRequest = {}): Promise<ModelListResponse> {
    return this.transport.request("model/list", params);
  }

  configGet(path: ConfigPath = "llm", target: { nodeId?: string } = {}): Promise<ConfigResponse> {
    return this.transport.request("config/get", { path, ...target });
  }

  configUpdate(value: ConfigValue, target: { nodeId?: string } = {}): Promise<ConfigResponse> {
    return this.transport.request("config/update", { value, ...target });
  }

  configDelete(path: ConfigPath, target: { nodeId?: string } = {}): Promise<ConfigDeleteResponse> {
    return this.transport.request("config/delete", { path, ...target });
  }

  getLlmConfig(): Promise<ConfigResponse> {
    return this.configGet("llm");
  }

  updateLlmConfig(config: LlmConfig): Promise<ConfigResponse> {
    return this.configUpdate({ type: "llm", value: config });
  }

  configReload(target: { nodeId?: string } = {}): Promise<ConfigReloadResponse> {
    return this.transport.request("config/reload", target);
  }

  /** Host settings and safe identity; enrollment returns no node credential. */
  readonly desktop = {
    workerIdentity: () => this.transport.request<AppProtocol.DesktopWorkerIdentityResponse>("desktop/workerIdentity", null),
    enrollWorker: (params: AppProtocol.DesktopWorkerEnrollParams) => this.transport.request<AppProtocol.DesktopWorkerIdentityResponse>("desktop/workerEnroll", params),
  };

  authLogout(): Promise<AuthLogoutResponse> {
    return this.transport.request("auth/logout", {});
  }

  startCloudLogin(params: AppProtocol.CloudLoginStartRequest): Promise<AppProtocol.CloudLoginStartResponse> {
    return this.transport.request("auth/login/start", params);
  }

  getCloudLoginStatus(params: AppProtocol.CloudLoginStatusRequest): Promise<AppProtocol.CloudLoginStatusResponse> {
    return this.transport.request("auth/login/status", params);
  }

  cancelCloudLogin(params: AppProtocol.CloudLoginCancelRequest): Promise<CloudLoginCancelResponse> {
    return this.transport.request("auth/login/cancel", params);
  }

  readAuthSession(): Promise<AuthSessionReadResponse> {
    return this.transport.request("auth/session/read", {});
  }

  clearAuthSession(): Promise<AuthSessionClearResponse> {
    return this.transport.request("auth/session/clear", {});
  }

  getCurrentUser(): Promise<UserCurrentResponse> {
    return this.transport.request("user/current", {});
  }

  readProfile(
    params: AppProtocol.ProfileReadRequest = {},
  ): Promise<ProfileReadResponse> {
    return this.transport.request("profile/read", params);
  }

  listPluginSources(
    params: AppProtocol.PluginSourceListRequest = {},
  ): Promise<AppProtocol.PluginSourceListResponse> {
    return this.transport.request("plugin/source/list", params);
  }

  addPluginSource(params: AppProtocol.PluginSourceAddRequest): Promise<AppProtocol.PluginSourceAddResponse> {
    return this.transport.request("plugin/source/add", params);
  }

  refreshPluginSource(
    params: AppProtocol.PluginSourceRefreshRequest,
  ): Promise<AppProtocol.PluginSourceRefreshResponse> {
    return this.transport.request("plugin/source/refresh", params);
  }

  removePluginSource(
    params: AppProtocol.PluginSourceRemoveRequest,
  ): Promise<AppProtocol.PluginSourceRemoveResponse> {
    return this.transport.request("plugin/source/remove", params);
  }

  searchPlugins(params: AppProtocol.PluginSearchRequest): Promise<AppProtocol.PluginSearchResponse> {
    return this.transport.request("plugin/search", params);
  }

  importMcpServer(params: AppProtocol.McpServerImportRequest): Promise<AppProtocol.PluginInstallResponse> { return this.transport.request("mcp/server/import", params); }

  startMcpAuthentication(params: AppProtocol.McpAuthenticationStartRequest): Promise<AppProtocol.McpAuthenticationStartResponse> { return this.transport.request("mcp/server/auth/start", params); }
  readMcpAuthentication(params: AppProtocol.McpAuthenticationReadRequest): Promise<AppProtocol.McpAuthenticationResponse> { return this.transport.request("mcp/server/auth/read", params); }

  updateMcpCredentials(params: AppProtocol.McpCredentialsUpdateRequest): Promise<AppProtocol.McpAuthenticationResponse> { return this.transport.request("mcp/server/credentials/update", params); }

  listMcpServers(params: AppProtocol.McpServerListRequest = {}): Promise<AppProtocol.McpServerListResponse> {
    return this.transport.request("mcp/server/list", params);
  }

  listPlugins(params: AppProtocol.PluginListRequest): Promise<AppProtocol.PluginListResponse> {
    return this.transport.request("plugin/list", params);
  }

  readPlugin(params: AppProtocol.PluginReadRequest): Promise<AppProtocol.PluginReadResponse> {
    return this.transport.request("plugin/read", params);
  }

  installPlugin(params: AppProtocol.PluginInstallRequest): Promise<AppProtocol.PluginInstallResponse> {
    return this.transport.request("plugin/install", params);
  }

  updatePlugin(params: AppProtocol.PluginUpdateRequest): Promise<AppProtocol.PluginUpdateResponse> {
    return this.transport.request("plugin/update", params);
  }

  uninstallPlugin(params: AppProtocol.PluginUninstallRequest): Promise<AppProtocol.PluginUninstallResponse> {
    return this.transport.request("plugin/uninstall", params);
  }

  enablePlugin(params: AppProtocol.PluginEnableRequest): Promise<AppProtocol.PluginEnableResponse> {
    return this.transport.request("plugin/enable", params);
  }

  disablePlugin(params: AppProtocol.PluginDisableRequest): Promise<AppProtocol.PluginDisableResponse> {
    return this.transport.request("plugin/disable", params);
  }

  listSkills(params: AppProtocol.SkillsListRequest = { include_disabled: false }): Promise<AppProtocol.SkillsListResponse> {
    return this.transport.request("skills/list", params);
  }

  refreshSkills(params: AppProtocol.SkillsRefreshRequest = {}): Promise<AppProtocol.SkillsListResponse> {
    return this.transport.request("skills/refresh", params);
  }

  suggestSkills(params: AppProtocol.SkillsSuggestRequest): Promise<AppProtocol.SkillsSuggestResponse> {
    return this.transport.request("skills/suggest", params);
  }

  readSkill(params: AppProtocol.SkillsReadRequest): Promise<AppProtocol.SkillsReadResponse> {
    return this.transport.request("skills/read", params);
  }

  resolveSkills(params: AppProtocol.SkillsResolveRequest): Promise<AppProtocol.SkillsResolveResponse> {
    return this.transport.request("skills/resolve", params);
  }

  listSessions(params: AppProtocol.SessionListRequest): Promise<SessionListResponse> {
    return this.transport.request("session/list", params);
  }

  getSession(params: AppProtocol.SessionGetRequest): Promise<AppProtocol.SessionGetResponse> {
    return this.transport.request("session/get", params);
  }

  getSessionById(sessionId: string): Promise<AppProtocol.SessionGetResponse> {
    return this.getSession({ sessionId });
  }

  patchSession(params: AppProtocol.SessionPatchRequest): Promise<SessionUpdateResponse> {
    return this.transport.request("session/patch", params);
  }

  deleteSession(params: AppProtocol.SessionDeleteRequest): Promise<SessionDeleteResponse> {
    return this.transport.request("session/delete", params);
  }

  deleteSessionById(sessionId: string): Promise<SessionDeleteResponse> {
    return this.deleteSession({ sessionId });
  }

  closeSession(params: AppProtocol.SessionCloseRequest): Promise<SessionCloseResponse> {
    return this.transport.request("session/close", params);
  }

  closeSessionById(sessionId: string): Promise<SessionCloseResponse> {
    return this.closeSession({ sessionId });
  }

  compactSession(params: AppProtocol.SessionCompactRequest): Promise<SessionCompactResponse> {
    return this.transport.request("session/compact", params);
  }

  compactSessionById(sessionId: string): Promise<SessionCompactResponse> {
    return this.compactSession({ sessionId });
  }

  clearSession(params: AppProtocol.SessionClearRequest): Promise<SessionClearResponse> {
    return this.transport.request("session/clear", params);
  }

  clearSessionById(sessionId: string): Promise<SessionClearResponse> {
    return this.clearSession({ sessionId });
  }

  getActiveTurn(params: AppProtocol.SessionActiveTurnRequest): Promise<SessionActiveTurnResponse> {
    return this.transport.request("session/active_turn", params);
  }

  getActiveTurnById(sessionId: string): Promise<SessionActiveTurnResponse> {
    return this.getActiveTurn({ sessionId });
  }

  getSessionHistory(params: AppProtocol.SessionHistoryRequest): Promise<SessionHistoryResponse> {
    return this.transport.request<SessionHistoryResponse>("session/history", params);
  }

  getSessionHistoryById(sessionId: string): Promise<SessionHistoryResponse> {
    return this.getSessionHistory({ sessionId, cursor: null, limit: null });
  }

  recoverTimeline(params: AppProtocol.RecoverTimelineRequest): Promise<RecoverTimelineResponse> {
    return this.transport.request<RecoverTimelineResponse>("timeline/recover", params);
  }

  recoverTimelineById(
    sessionId: string,
    afterEventId: string | null = null,
    replayLimit: number | null = null,
  ): Promise<RecoverTimelineResponse> {
    return this.recoverTimeline({
      sessionId,
      afterEventId,
      replayLimit,
    });
  }

  sync = {
    metrics: (): Promise<AppProtocol.SyncMetricsResponse> => this.transport.request("sync/metrics", null),
    status: (): Promise<AppProtocol.SyncStatusResponse> => this.transport.request("sync/status", null),
    enable: (request: AppProtocol.SyncEnableRequest = { claimLocalHistory: false }): Promise<AppProtocol.SyncStatusResponse> => this.transport.request("sync/enable", request),
    disable: (): Promise<AppProtocol.SyncStatusResponse> => this.transport.request("sync/disable", null),
  };

  computers = {
    delete: (params: AppProtocol.DeleteComputerRequest): Promise<AppProtocol.DeleteComputerResponse> =>
      this.transport.request("computer/delete", params),
    list: (params: AppProtocol.ComputerListRequest = {}): Promise<AppProtocol.ComputerListResponse> =>
      this.transport.request("computer/list", params),
    read: (params: AppProtocol.GetNodeRequest): Promise<AppProtocol.NodeInfo> =>
      this.transport.request("computer/read", params),
    create: (params: AppProtocol.CreateNodeRequest): Promise<AppProtocol.CreatedNode> =>
      this.transport.request("computer/create", params),
    rename: (params: AppProtocol.RenameComputerRequest): Promise<AppProtocol.NodeInfo> =>
      this.transport.request("computer/rename", params),
    setStatus: (params: AppProtocol.SetComputerStatusRequest): Promise<AppProtocol.NodeInfo> =>
      this.transport.request("computer/setStatus", params),
    rotateCredential: (params: AppProtocol.RotateCredentialRequest): Promise<AppProtocol.NodeCredential> =>
      this.transport.request("computer/rotateCredential", params),
  };

  listProjects(params: AppProtocol.ProjectListRequest = {}): Promise<ProjectListResponse> {
    return this.transport.request<ProjectListResponse>("project/list", params);
  }

  getProject(params: AppProtocol.ProjectReadRequest): Promise<ProjectReadResponse> {
    return this.transport.request<ProjectReadResponse>("project/read", params);
  }

  getProjectById(projectId: string): Promise<ProjectReadResponse> {
    return this.getProject({ project_id: projectId });
  }

  createProject(params: AppProtocol.ProjectCreateRequest): Promise<ProjectCreateResponse> {
    return this.transport.request<ProjectCreateResponse>("project/create", params);
  }

  createProjectFromFields(name: string, directory: string): Promise<ProjectCreateResponse> {
    return this.createProject({ name, directory });
  }

  patchProject(params: AppProtocol.ProjectPatchRequest): Promise<ProjectPatchResponse> {
    return this.transport.request<ProjectPatchResponse>("project/patch", params);
  }

  patchProjectById(projectId: string, patch: ProjectPatch): Promise<ProjectPatchResponse> {
    return this.patchProject({ project_id: projectId, name: patch.name });
  }

  deleteProject(params: AppProtocol.ProjectDeleteRequest): Promise<ProjectDeleteResponse> {
    return this.transport.request("project/delete", params);
  }

  deleteProjectById(projectId: string): Promise<ProjectDeleteResponse> {
    return this.deleteProject({ project_id: projectId });
  }

  readFsDirectory(params: AppProtocol.FsDirectoryReadRequest = { path: null }): Promise<FsDirectoryReadResponse> {
    return this.transport.request("fs/directory/read", params);
  }

  createFsDirectory(params: AppProtocol.FsDirectoryCreateRequest): Promise<FsDirectoryCreateResponse> {
    return this.transport.request("fs/directory/create", params);
  }

  startTurn(params: AppProtocol.StartTurnRequest): Promise<AppProtocol.StartTurnResponse> {
    return this.transport.request("turn/start", params);
  }

  retryTurn(params: AppProtocol.RetryTurnRequest): Promise<AppProtocol.StartTurnResponse> {
    return this.transport.request("turn/retry", params);
  }

  interruptTurn(params: AppProtocol.InterruptTurnRequest): Promise<AppProtocol.InterruptTurnResponse> {
    return this.transport.request("turn/interrupt", params);
  }

  interruptTurnById(sessionId: string, turnId: string): Promise<AppProtocol.InterruptTurnResponse> {
    return this.interruptTurn({
      identity: { sessionId, turnId },
    });
  }

  submitTurnInput(
    params: AppProtocol.SubmitTurnInputRequest,
  ): Promise<AppProtocol.SubmitTurnInputResponse> {
    return this.transport.request("turn/input", params);
  }

  messageQueue = {
    add: (params: AppProtocol.MessageQueueAddRequest): Promise<AppProtocol.MessageQueue> =>
      this.transport.request("messageQueue/add", params),
    list: (params: AppProtocol.MessageQueueListRequest): Promise<AppProtocol.MessageQueue> =>
      this.transport.request("messageQueue/list", params),
    update: (params: AppProtocol.MessageQueueUpdateRequest): Promise<AppProtocol.MessageQueue> =>
      this.transport.request("messageQueue/update", params),
    delete: (params: AppProtocol.MessageQueueDeleteRequest): Promise<AppProtocol.MessageQueue> =>
      this.transport.request("messageQueue/delete", params),
    reorder: (params: AppProtocol.MessageQueueReorderRequest): Promise<AppProtocol.MessageQueue> =>
      this.transport.request("messageQueue/reorder", params),
    start: (params: AppProtocol.MessageQueueStartRequest): Promise<AppProtocol.StartTurnResponse> =>
      this.transport.request("messageQueue/start", params),
  };

  subscribeEvents(
    params: AppProtocol.SubscribeTimelineRequest,
  ): Promise<AppProtocol.SubscribeTimelineResponse> {
    return this.transport.request("event/subscribe", params);
  }

  unsubscribeEvents(
    subscriptionId: string,
  ): Promise<AppProtocol.EventUnsubscribeResponse> {
    return this.transport.request("event/unsubscribe", { subscriptionId });
  }

  listMemoryFiles(request: MemoryFileListRequest = {}): Promise<MemoryFileListResponse> {
    return this.transport.request<MemoryFileListResponse>("memory/file/list", request);
  }

  readMemoryFile(request: MemoryFileReadRequest): Promise<MemoryFileReadResponse> {
    return this.transport.request("memory/file/read", request);
  }

  searchMemory(request: MemorySearchRequest): Promise<MemorySearchResponse> {
    return this.transport.request<MemorySearchResponse>("memory/search", request);
  }

  getMemory(request: MemoryGetRequest): Promise<MemoryGetResponse> {
    return this.transport.request<MemoryGetResponse>("memory/get", request);
  }

  writeMemoryFile(request: MemoryFileWriteRequest): Promise<MemoryFileWriteResponse> {
    return this.transport.request("memory/file/write", request);
  }

  workspaceInfo(params: AppProtocol.WorkspaceInfoRequest = {}): Promise<WorkspaceInfoResponse> {
    return this.transport.request<WorkspaceInfoResponse>("workspace/info", params);
  }

  listWorkspaceDocs(params: AppProtocol.WorkspaceDocListRequest = {}): Promise<WorkspaceDocListResponse> {
    return this.transport.request<WorkspaceDocListResponse>("workspace/doc/list", params);
  }

  readWorkspaceDoc(kind: WorkspaceDocKind, target: AppProtocol.WorkspaceInfoRequest = {}): Promise<WorkspaceDocReadResponse> {
    return this.transport.request("workspace/doc/read", { kind, ...target });
  }

  writeWorkspaceDoc(
    kind: WorkspaceDocKind,
    content: string,
    expectedVersion: string | null,
    target: AppProtocol.WorkspaceInfoRequest = {},
  ): Promise<WorkspaceDocWriteResponse> {
    return this.transport.request("workspace/doc/write", {
      kind,
      content,
      expected_version: expectedVersion,
      ...target,
    });
  }
}

function throwIfUploadAborted(signal: AbortSignal | undefined): void {
  if (!signal?.aborted) {
    return;
  }
  throw signal.reason instanceof Error
    ? signal.reason
    : new DOMException("The operation was aborted.", "AbortError");
}

function encodeBase64(data: Uint8Array): string {
  let binary = "";
  const chunkSize = 0x8000;
  for (let offset = 0; offset < data.length; offset += chunkSize) {
    binary += String.fromCharCode(...data.subarray(offset, offset + chunkSize));
  }
  return globalThis.btoa(binary);
}
