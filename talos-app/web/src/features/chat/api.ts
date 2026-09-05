import { AppProtocol, TalosApiError } from "@talos/sdk-browser";
import type {
  ModelInfo,
  ModelSelection,
  AgentUserContent,
  AgentUserInput,
  ProjectInfo as ApiProjectInfo,
  RetryTurnRequest,
  StartTurnRequest,
  SessionInfo,
  SessionEventStreamOptions,
  SessionHistoryResponse,
  SessionPatchRequest,
  SubmitTurnInputRequest,
  SubmitTurnInputResponse,
  TimelineEvent,
  UserPermissionMode,
} from "@talos/sdk-browser";

import { getTalosClient } from "@/shared/api/client";
import { isUnboundProject } from "@/shared/types/domain";
import type { ProjectView } from "@/shared/types/ui";

const ACTIVE_TURN_RUNNING_CODE = AppProtocol.ErrorCode.ActiveTurnRunning;
const PROJECT_DELETE_CONFLICT_CODE = AppProtocol.ErrorCode.Conflict;

export function isActiveTurnRunningError(error: unknown) {
  return error instanceof TalosApiError && error.code === ACTIVE_TURN_RUNNING_CODE;
}

function mapProjectView(project: ApiProjectInfo): ProjectView {
  return {
    id: project.project_id,
    name: project.name,
    directory: project.directory,
    status: project.status,
    createdAt: project.created_at,
    updatedAt: project.updated_at,
    origin: "server",
    sessionCount: project.session_count,
  };
}

function deriveProjectName(directory: string) {
  const normalized = directory.trim().replace(/\/+$/, "");
  return normalized.split("/").filter(Boolean).at(-1) || "project";
}

interface CreateProjectInput {
  name?: string;
  directory: string;
}

export async function listModels() {
  return getTalosClient().models.list();
}

export async function listProjects() {
  const response = await getTalosClient().projects.list({});
  return response.projects.map(mapProjectView);
}

export async function listSessions() {
  const response = await getTalosClient().sessions.list();
  return response.sessions;
}

export async function getSession(sessionId: string) {
  const response = await getTalosClient().sessions.get({ sessionId });
  return response.session;
}

export function generateSessionId() {
  if (typeof globalThis.crypto?.randomUUID === "function") {
    return globalThis.crypto.randomUUID();
  }
  return `${Date.now()}-${Math.random().toString(36).slice(2)}`;
}

interface InitialTurnOptions {
  fileIds?: string[];
  modelSelection?: ModelSelection;
  project?: ProjectView;
  userPermissionMode?: UserPermissionMode;
}

export function createInitialTurnRequest(
  sessionId: string,
  text: string,
  options: InitialTurnOptions = {},
): StartTurnRequest {
  const fileIds = options.fileIds ?? [];
  const content: AgentUserContent[] = [
    ...(text ? [{ type: "text" as const, text }] : []),
    ...fileIds.map((fileId) => ({ type: "file" as const, fileId })),
  ];
  const input: AgentUserInput = { content };
  return {
    sessionId,
    input,
    modelSelection: options.modelSelection ?? null,
    userPermissionMode: options.userPermissionMode ?? null,
    projectId: options.project && !isUnboundProject(options.project.id)
      ? options.project.id
      : null,
    currentDirectory: null,
    clientRequestId: null,
  };
}

export async function suggestSkills(query: string, limit = 8) {
  const response = await getTalosClient().skills.suggest({ query, limit });
  return response.data;
}

export async function createProject(input: string | CreateProjectInput) {
  const directory = typeof input === "string" ? input : input.directory;
  const name = typeof input === "string" ? deriveProjectName(directory) : input.name?.trim() || deriveProjectName(directory);
  const response = await getTalosClient().projects.create({
    name,
    directory,
  });
  return mapProjectView(response.project);
}

export async function readFsDirectory(path?: string | null) {
  const response = await getTalosClient().fs.directory.read({ path: path ?? null });
  return response.directory;
}

export async function createFsDirectory(parent: string, name: string) {
  return getTalosClient().fs.directory.create({ parent, name });
}

export async function deleteProject(projectId: string) {
  try {
    await getTalosClient().projects.delete({ project_id: projectId });
  } catch (error) {
    if (error instanceof TalosApiError && error.code === PROJECT_DELETE_CONFLICT_CODE) {
      throw new Error("PROJECT_DELETE_CONFLICT");
    }
    throw error;
  }
}

export async function updateProjectName(projectId: string, name: string) {
  const response = await getTalosClient().projects.patch({
    project_id: projectId,
    name,
  });
  return mapProjectView(response.project);
}

export async function updateSessionModel(sessionId: string, modelSelection: ModelSelection) {
  const request: SessionPatchRequest = {
    sessionId,
    conf: {
      modelSelection,
      userPermissionMode: null,
    },
    currentDirectory: null,
  };
  await getTalosClient().sessions.patch(request);
}

export async function updateSessionPermissionMode(
  sessionId: string,
  userPermissionMode: NonNullable<SessionInfo["effectiveMode"]>,
) {
  const request: SessionPatchRequest = {
    sessionId,
    conf: {
      modelSelection: null,
      userPermissionMode,
    },
    currentDirectory: null,
  };
  const response = await getTalosClient().sessions.patch(request);
  return response.session;
}

export async function submitTurnInput(
  request: SubmitTurnInputRequest,
): Promise<SubmitTurnInputResponse> {
  return getTalosClient().turns.input(request);
}

export async function closeSession(sessionId: string) {
  await getTalosClient().sessions.close({ sessionId });
}

export async function deleteSession(sessionId: string) {
  await getTalosClient().sessions.delete({ sessionId });
}

export async function cancelActiveTurn(sessionId: string, turnId: string) {
  await getTalosClient().sessions.cancel({
    identity: { sessionId, turnId },
  });
}

export async function compactSession(sessionId: string) {
  const response = await getTalosClient().sessions.compact({ sessionId });
  return response.session;
}

export async function clearSession(sessionId: string) {
  const response = await getTalosClient().sessions.clear({ sessionId });
  return response.session;
}

export async function getActiveTurn(sessionId: string) {
  const response = await getTalosClient().sessions.activeTurn({ sessionId });
  return response.activeTurn;
}

export async function getSessionHistory(sessionId: string) {
  return getTalosClient().sessions.history({ sessionId, cursor: null, limit: null });
}

export async function recoverTimelineLiveTail(sessionId: string, afterEventId?: string) {
  const response = await getTalosClient().sessions.recoverTimeline({
    sessionId,
    afterEventId: afterEventId ?? null,
    replayLimit: null,
  });
  return response;
}

export async function startTimelineTurn(
  request: StartTurnRequest,
  onAcceptedMessageId?: (messageId: string) => void,
): Promise<string> {
  const client = getTalosClient();
  const ack = await client.turns.start(request);
  if (!ack.messageId) {
    throw new Error(`Message ack did not include messageId: ${ack.turnId}`);
  }
  onAcceptedMessageId?.(ack.messageId);
  return ack.messageId;
}

export async function retryTimelineTurn(
  request: RetryTurnRequest,
  onAcceptedMessageId?: (messageId: string) => void,
): Promise<string> {
  const client = getTalosClient();
  const ack = await client.turns.retry(request);
  if (!ack.messageId) {
    throw new Error(`Retry ack did not include messageId: ${ack.turnId}`);
  }
  onAcceptedMessageId?.(ack.messageId);
  return ack.messageId;
}

export async function subscribeTimelineEvents(
  sessionId: string,
  onEvent: (event: TimelineEvent) => void,
  signal?: AbortSignal,
  afterEventId?: string,
  options?: SessionEventStreamOptions,
) {
  return getTalosClient().turns.events(
    sessionId,
    afterEventId,
    onEvent,
    signal,
    { ...options, persistent: true },
  );
}

export function belongsToProject(session: SessionInfo, project?: ProjectView) {
  if (!project || isUnboundProject(project.id)) {
    return !session.projectId;
  }
  return session.projectId === project.id;
}

export type ChatApiModel = ModelInfo;
export type ChatApiSession = SessionInfo;
export type ChatApiActiveTurn = Awaited<ReturnType<typeof getActiveTurn>>;
export type ChatApiSessionHistory = SessionHistoryResponse;
export type ChatApiSessionDetail = SessionInfo;
