import type * as AppProtocol from "./generated/app-protocol";
import type {
  MemoryFileListResponse,
  MemoryFileReadRequest,
  MemoryFileReadResponse,
  MemoryFileWriteRequest,
  MemoryFileWriteResponse,
  MemoryFileListRequest,
  MemorySearchRequest,
  MemorySearchResponse,
  FsDirectoryCreateResponse,
  FsDirectoryReadResponse,
  ConfigGetResponse,
  ConfigReloadResponse,
  ConfigUpdateResponse,
  ConfigValue,
  LlmConfig,
  ModelListResponse,
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
  TimelineRecoveryResponse,
  UserCurrentResponse,
  AuthLogoutResponse,
  AuthSessionClearResponse,
  AuthSessionReadResponse,
  CloudLoginCancelResponse,
  SessionCloseResponse,
  WorkspaceInfoResponse,
  WorkspaceDocKind,
  WorkspaceDocListResponse,
  WorkspaceDocReadResponse,
  WorkspaceDocWriteResponse,
} from "./types";

export type AppServerEventHandler = (
  event: AppProtocol.TimelineEventEnvelope,
) => void;

export type AppServerSubscriptionTerminatedHandler = (
  envelope: AppProtocol.SubscriptionTerminatedEnvelope,
) => void;

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
  onSubscriptionTerminated?(handler: AppServerSubscriptionTerminatedHandler): () => void;
  onError?(handler: AppServerErrorHandler): () => void;
}

export class AppServerClient {
  constructor(protected readonly transport: AppServerTransport) {}

  onEvent(handler: AppServerEventHandler): () => void {
    return this.transport.onEvent(handler);
  }

  onSubscriptionTerminated(handler: AppServerSubscriptionTerminatedHandler): () => void {
    return this.transport.onSubscriptionTerminated?.(handler) ?? (() => undefined);
  }

  onError(handler: AppServerErrorHandler): () => void {
    return this.transport.onError?.(handler) ?? (() => undefined);
  }

  listModels(): Promise<ModelListResponse> {
    return this.transport.request("model.list", {} satisfies AppProtocol.ModelListRequest);
  }

  configGet(): Promise<ConfigGetResponse> {
    return this.transport.request("config.get", {});
  }

  configUpdate(path: string, value: ConfigValue): Promise<ConfigUpdateResponse> {
    return this.transport.request("config.update", { path, value });
  }

  getLlmConfig(): Promise<ConfigGetResponse> {
    return this.configGet();
  }

  updateLlmConfig(config: LlmConfig): Promise<ConfigUpdateResponse> {
    return this.configUpdate("llm", { type: "llm", value: config });
  }

  configReload(): Promise<ConfigReloadResponse> {
    return this.transport.request("config.reload", {});
  }

  authLogout(): Promise<AuthLogoutResponse> {
    return this.transport.request("auth.logout", {});
  }

  startCloudLogin(params: AppProtocol.CloudLoginStartRequest): Promise<AppProtocol.CloudLoginStartResponse> {
    return this.transport.request("auth.login.start", params);
  }

  getCloudLoginStatus(params: AppProtocol.CloudLoginStatusRequest): Promise<AppProtocol.CloudLoginStatusResponse> {
    return this.transport.request("auth.login.status", params);
  }

  cancelCloudLogin(params: AppProtocol.CloudLoginCancelRequest): Promise<CloudLoginCancelResponse> {
    return this.transport.request("auth.login.cancel", params);
  }

  readAuthSession(): Promise<AuthSessionReadResponse> {
    return this.transport.request("auth.session.read", {});
  }

  clearAuthSession(): Promise<AuthSessionClearResponse> {
    return this.transport.request("auth.session.clear", {});
  }

  getCurrentUser(): Promise<UserCurrentResponse> {
    return this.transport.request("user.current", {});
  }

  listPluginSources(
    params: AppProtocol.PluginSourceListRequest = {},
  ): Promise<AppProtocol.PluginSourceListResponse> {
    return this.transport.request("plugin.source.list", params);
  }

  addPluginSource(params: AppProtocol.PluginSourceAddRequest): Promise<AppProtocol.PluginSourceAddResponse> {
    return this.transport.request("plugin.source.add", params);
  }

  refreshPluginSource(
    params: AppProtocol.PluginSourceRefreshRequest,
  ): Promise<AppProtocol.PluginSourceRefreshResponse> {
    return this.transport.request("plugin.source.refresh", params);
  }

  removePluginSource(
    params: AppProtocol.PluginSourceRemoveRequest,
  ): Promise<AppProtocol.PluginSourceRemoveResponse> {
    return this.transport.request("plugin.source.remove", params);
  }

  searchPlugins(params: AppProtocol.PluginSearchRequest): Promise<AppProtocol.PluginSearchResponse> {
    return this.transport.request("plugin.search", params);
  }

  listPlugins(params: AppProtocol.PluginListRequest): Promise<AppProtocol.PluginListResponse> {
    return this.transport.request("plugin.list", params);
  }

  readPlugin(params: AppProtocol.PluginReadRequest): Promise<AppProtocol.PluginReadResponse> {
    return this.transport.request("plugin.read", params);
  }

  installPlugin(params: AppProtocol.PluginInstallRequest): Promise<AppProtocol.PluginInstallResponse> {
    return this.transport.request("plugin.install", params);
  }

  updatePlugin(params: AppProtocol.PluginUpdateRequest): Promise<AppProtocol.PluginUpdateResponse> {
    return this.transport.request("plugin.update", params);
  }

  uninstallPlugin(params: AppProtocol.PluginUninstallRequest): Promise<AppProtocol.PluginUninstallResponse> {
    return this.transport.request("plugin.uninstall", params);
  }

  enablePlugin(params: AppProtocol.PluginEnableRequest): Promise<AppProtocol.PluginEnableResponse> {
    return this.transport.request("plugin.enable", params);
  }

  disablePlugin(params: AppProtocol.PluginDisableRequest): Promise<AppProtocol.PluginDisableResponse> {
    return this.transport.request("plugin.disable", params);
  }

  listSkills(params: AppProtocol.SkillsListRequest = { include_disabled: false }): Promise<AppProtocol.SkillsListResponse> {
    return this.transport.request("skills.list", params);
  }

  refreshSkills(params: AppProtocol.SkillsRefreshRequest = {}): Promise<AppProtocol.SkillsListResponse> {
    return this.transport.request("skills.refresh", params);
  }

  suggestSkills(params: AppProtocol.SkillsSuggestRequest): Promise<AppProtocol.SkillsSuggestResponse> {
    return this.transport.request("skills.suggest", params);
  }

  readSkill(params: AppProtocol.SkillsReadRequest): Promise<AppProtocol.SkillsReadResponse> {
    return this.transport.request("skills.read", params);
  }

  resolveSkills(params: AppProtocol.SkillsResolveRequest): Promise<AppProtocol.SkillsResolveResponse> {
    return this.transport.request("skills.resolve", params);
  }

  listSessions(params: AppProtocol.SessionListRequest): Promise<SessionListResponse> {
    return this.transport.request("session.list", params);
  }

  getSession(params: AppProtocol.SessionGetRequest): Promise<AppProtocol.SessionGetResponse> {
    return this.transport.request("session.get", params);
  }

  getSessionById(sessionId: string): Promise<AppProtocol.SessionGetResponse> {
    return this.getSession({ session_id: sessionId });
  }

  patchSession(params: AppProtocol.SessionPatchRequest): Promise<SessionUpdateResponse> {
    return this.transport.request("session.patch", params);
  }

  deleteSession(params: AppProtocol.SessionDeleteRequest): Promise<SessionDeleteResponse> {
    return this.transport.request("session.delete", params);
  }

  deleteSessionById(sessionId: string): Promise<SessionDeleteResponse> {
    return this.deleteSession({ session_id: sessionId });
  }

  closeSession(params: AppProtocol.SessionCloseRequest): Promise<SessionCloseResponse> {
    return this.transport.request("session.close", params);
  }

  closeSessionById(sessionId: string): Promise<SessionCloseResponse> {
    return this.closeSession({ session_id: sessionId });
  }

  compactSession(params: AppProtocol.SessionCompactRequest): Promise<SessionCompactResponse> {
    return this.transport.request("session.compact", params);
  }

  compactSessionById(sessionId: string): Promise<SessionCompactResponse> {
    return this.compactSession({ session_id: sessionId });
  }

  clearSession(params: AppProtocol.SessionClearRequest): Promise<SessionClearResponse> {
    return this.transport.request("session.clear", params);
  }

  clearSessionById(sessionId: string): Promise<SessionClearResponse> {
    return this.clearSession({ session_id: sessionId });
  }

  getActiveTurn(params: AppProtocol.SessionActiveTurnRequest): Promise<SessionActiveTurnResponse> {
    return this.transport.request("session.active_turn", params);
  }

  getActiveTurnById(sessionId: string): Promise<SessionActiveTurnResponse> {
    return this.getActiveTurn({ session_id: sessionId });
  }

  getSessionHistory(params: AppProtocol.SessionHistoryRequest): Promise<SessionHistoryResponse> {
    return this.transport.request<SessionHistoryResponse>("session.history", {
      session_id: params.session_id,
    });
  }

  getSessionHistoryById(sessionId: string): Promise<SessionHistoryResponse> {
    return this.getSessionHistory({ session_id: sessionId });
  }

  recoverTimeline(params: AppProtocol.RecoverTimelineRequest): Promise<TimelineRecoveryResponse> {
    return this.transport.request<TimelineRecoveryResponse>("timeline.recover", params);
  }

  recoverTimelineById(
    sessionId: string,
    afterEventId: string | null = null,
    replayLimit: number | null = null,
  ): Promise<TimelineRecoveryResponse> {
    return this.recoverTimeline({
      session_id: sessionId,
      after_event_id: afterEventId,
      replay_limit: replayLimit,
    });
  }

  listProjects(params: AppProtocol.ProjectListRequest = {}): Promise<ProjectListResponse> {
    return this.transport.request<ProjectListResponse>("project.list", params);
  }

  getProject(params: AppProtocol.ProjectReadRequest): Promise<ProjectReadResponse> {
    return this.transport.request<ProjectReadResponse>("project.read", params);
  }

  getProjectById(projectId: string): Promise<ProjectReadResponse> {
    return this.getProject({ project_id: projectId });
  }

  createProject(params: AppProtocol.ProjectCreateRequest): Promise<ProjectCreateResponse> {
    return this.transport.request<ProjectCreateResponse>("project.create", params);
  }

  createProjectFromFields(name: string, directory: string): Promise<ProjectCreateResponse> {
    return this.createProject({ name, directory });
  }

  patchProject(params: AppProtocol.ProjectPatchRequest): Promise<ProjectPatchResponse> {
    return this.transport.request<ProjectPatchResponse>("project.patch", params);
  }

  patchProjectById(projectId: string, patch: ProjectPatch): Promise<ProjectPatchResponse> {
    return this.patchProject({ project_id: projectId, name: patch.name });
  }

  deleteProject(params: AppProtocol.ProjectDeleteRequest): Promise<ProjectDeleteResponse> {
    return this.transport.request("project.delete", params);
  }

  deleteProjectById(projectId: string): Promise<ProjectDeleteResponse> {
    return this.deleteProject({ project_id: projectId });
  }

  readFsDirectory(params: AppProtocol.FsDirectoryReadRequest = { path: null }): Promise<FsDirectoryReadResponse> {
    return this.transport.request("fs.directory.read", params);
  }

  createFsDirectory(params: AppProtocol.FsDirectoryCreateRequest): Promise<FsDirectoryCreateResponse> {
    return this.transport.request("fs.directory.create", params);
  }

  startTurn(params: AppProtocol.StartTurnRequest): Promise<AppProtocol.StartTurnResponse> {
    return this.transport.request("turn.start", params);
  }

  retryTurn(params: AppProtocol.RetryTurnRequest): Promise<AppProtocol.RetryTurnResponse> {
    return this.transport.request("turn.retry", params);
  }

  interruptTurn(params: AppProtocol.InterruptTurnRequest): Promise<AppProtocol.InterruptTurnResponse> {
    return this.transport.request("turn.interrupt", params);
  }

  interruptTurnById(sessionId: string): Promise<AppProtocol.InterruptTurnResponse> {
    return this.interruptTurn({ session_id: sessionId });
  }

  subscribeEvents(
    params: AppProtocol.SubscribeTimelineRequest,
  ): Promise<AppProtocol.SubscribeTimelineResponse> {
    return this.transport.request("event.subscribe", params);
  }

  unsubscribeEvents(
    subscriptionId: string,
  ): Promise<AppProtocol.EventUnsubscribeResponse> {
    return this.transport.request("event.unsubscribe", { subscription_id: subscriptionId });
  }

  submitDynamicToolResult(
    params: AppProtocol.SubmitDynamicToolResultRequest,
  ): Promise<AppProtocol.SubmitDynamicToolResultResponse> {
    return this.transport.request("tool.dynamic_result.submit", params);
  }

  listMemoryFiles(request: MemoryFileListRequest = {}): Promise<MemoryFileListResponse> {
    return this.transport.request<MemoryFileListResponse>("memory.file.list", request);
  }

  readMemoryFile(request: MemoryFileReadRequest): Promise<MemoryFileReadResponse> {
    return this.transport.request("memory.file.read", request);
  }

  searchMemory(request: MemorySearchRequest): Promise<MemorySearchResponse> {
    return this.transport.request<MemorySearchResponse>("memory.search", request);
  }

  writeMemoryFile(request: MemoryFileWriteRequest): Promise<MemoryFileWriteResponse> {
    return this.transport.request("memory.file.write", request);
  }

  listWorkspaceDocs(): Promise<WorkspaceDocListResponse> {
    return this.transport.request<WorkspaceDocListResponse>("workspace.doc.list", {});
  }

  getWorkspaceInfo(): Promise<WorkspaceInfoResponse> {
    return this.transport.request("workspace.info", {});
  }

  readWorkspaceDoc(kind: WorkspaceDocKind): Promise<WorkspaceDocReadResponse> {
    return this.transport.request("workspace.doc.read", { kind });
  }

  writeWorkspaceDoc(
    kind: WorkspaceDocKind,
    content: string,
    expectedVersion: string | null,
  ): Promise<WorkspaceDocWriteResponse> {
    return this.transport.request("workspace.doc.write", {
      kind,
      content,
      expected_version: expectedVersion,
    });
  }
}
