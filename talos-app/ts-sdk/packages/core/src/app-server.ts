import type * as AppProtocol from "./generated/app-protocol";
import type {
  MemoryFileDetail,
  MemoryFileSummary,
  MemorySearchResult,
  ModelInfo,
  ProjectPatch,
  ProjectRecord,
  TalosMessageResponse,
  TalosSession,
  WorkspaceDocDetail,
  WorkspaceDocKind,
  WorkspaceDocSummary,
} from "./types";

export type AppServerEventHandler = (
  envelope: AppProtocol.ApplicationEventEnvelope,
) => void;

export interface AppServerTransport {
  request<T>(method: AppProtocol.ClientMethod, params: unknown): Promise<T>;
  onEvent(handler: AppServerEventHandler): () => void;
}

interface ListResponse<T> {
  data: T[];
}

export class AppServerClient {
  constructor(protected readonly transport: AppServerTransport) {}

  onEvent(handler: AppServerEventHandler): () => void {
    return this.transport.onEvent(handler);
  }

  listModels(): Promise<ModelInfo[]> {
    return this.transport
      .request<ListResponse<ModelInfo>>("model.list", {})
      .then((response) => response.data);
  }

  configRead(): Promise<unknown> {
    return this.transport.request("config.read", {});
  }

  createSession(params: AppProtocol.SessionCreateParams): Promise<TalosSession> {
    return this.transport.request("session.create", params);
  }

  listSessions(params: AppProtocol.SessionListParams): Promise<TalosSession[]> {
    return this.transport
      .request<ListResponse<TalosSession>>("session.list", params)
      .then((response) => response.data);
  }

  getSession(sessionId: string): Promise<TalosSession> {
    return this.transport.request("session.read", { session_id: sessionId });
  }

  patchSession(params: AppProtocol.SessionPatchParams): Promise<unknown> {
    return this.transport.request("session.patch", params);
  }

  deleteSession(sessionId: string): Promise<unknown> {
    return this.transport.request("session.delete", { session_id: sessionId });
  }

  getSessionTurns(sessionId: string): Promise<TalosMessageResponse[]> {
    return this.transport
      .request<ListResponse<TalosMessageResponse>>("session.turns", { session_id: sessionId })
      .then((response) => response.data);
  }

  listProjects(): Promise<ProjectRecord[]> {
    return this.transport
      .request<ListResponse<ProjectRecord>>("project.list", {})
      .then((response) => response.data);
  }

  getProject(projectId: string): Promise<ProjectRecord> {
    return this.transport.request("project.read", { project_id: projectId });
  }

  createProject(name: string, directory: string): Promise<ProjectRecord> {
    return this.transport.request("project.create", { name, directory });
  }

  patchProject(projectId: string, patch: ProjectPatch): Promise<ProjectRecord> {
    return this.transport.request("project.patch", {
      project_id: projectId,
      name: patch.name ?? null,
    });
  }

  deleteProject(projectId: string): Promise<unknown> {
    return this.transport.request("project.delete", { project_id: projectId });
  }

  startTurn(params: AppProtocol.TurnStartParams): Promise<AppProtocol.TurnStartResponse> {
    return this.transport.request("turn.start", params);
  }

  interruptTurn(sessionId: string): Promise<unknown> {
    return this.transport.request("turn.interrupt", { session_id: sessionId });
  }

  subscribeEvents(
    params: AppProtocol.EventSubscribeParams,
  ): Promise<AppProtocol.EventSubscribeResponse> {
    return this.transport.request("event.subscribe", params);
  }

  submitToolApproval(params: AppProtocol.ToolApprovalSubmitParams): Promise<unknown> {
    return this.transport.request("tool.approval.submit", params);
  }

  listMemoryFiles(limit?: number): Promise<MemoryFileSummary[]> {
    return this.transport
      .request<ListResponse<MemoryFileSummary>>("memory.file.list", { limit: limit ?? null })
      .then((response) => response.data);
  }

  readMemoryFile(path: string): Promise<MemoryFileDetail> {
    return this.transport.request("memory.file.read", { path });
  }

  searchMemory(query: string, limit?: number): Promise<MemorySearchResult[]> {
    return this.transport
      .request<ListResponse<MemorySearchResult>>("memory.search", {
        query,
        limit: limit ?? null,
      })
      .then((response) => response.data);
  }

  writeMemoryFile(
    path: string,
    content: string,
    expectedVersion: string | null,
  ): Promise<MemoryFileDetail> {
    return this.transport.request("memory.file.write", {
      path,
      content,
      expected_version: expectedVersion,
    });
  }

  listWorkspaceDocs(): Promise<WorkspaceDocSummary[]> {
    return this.transport
      .request<ListResponse<WorkspaceDocSummary>>("workspace.doc.list", {})
      .then((response) => response.data);
  }

  readWorkspaceDoc(kind: WorkspaceDocKind): Promise<WorkspaceDocDetail> {
    return this.transport.request("workspace.doc.read", { kind });
  }

  writeWorkspaceDoc(
    kind: WorkspaceDocKind,
    content: string,
    expectedVersion: string | null,
  ): Promise<WorkspaceDocDetail> {
    return this.transport.request("workspace.doc.write", {
      kind,
      content,
      expected_version: expectedVersion,
    });
  }
}
