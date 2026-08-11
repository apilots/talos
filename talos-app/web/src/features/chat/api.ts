import { AppProtocol, TalosApiError } from "@talos/sdk-browser";
import type {
  ModelInfo,
  ModelSelection,
  ProjectInfo as ApiProjectInfo,
  RetryTurnRequest,
  SkillSuggestion,
  StartTurnRequest,
  SessionInfo,
  SessionPatchRequest,
  TalosContext,
  TalosInputItem,
  TalosSessionHistorySnapshot,
  TimelineEventEnvelope,
} from "@talos/sdk-browser";

import { getTalosClient } from "@/shared/api/client";
import { isUnboundProject } from "@/shared/types/domain";
import type { ProjectView } from "@/shared/types/ui";

const ACTIVE_TURN_RUNNING_CODE = AppProtocol.ErrorCode.ActiveTurnRunning;
const PROJECT_DELETE_CONFLICT_CODE = AppProtocol.ErrorCode.Conflict;

export function isActiveTurnRunningError(error: unknown) {
  return error instanceof TalosApiError && error.code === ACTIVE_TURN_RUNNING_CODE;
}

function createContext(userId: string): TalosContext {
  const randomPart = Math.random().toString(36).slice(2, 10);
  return {
    source_type: "http",
    provider: "sdk",
    user_id: userId,
    message_id: `msg_${randomPart}`,
    trace_id: `trace_${randomPart}`,
    channel_id: "web",
    thread_id: null,
    project_root: null,
    environment: null,
  };
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
  const response = await getTalosClient().sessions.get({ session_id: sessionId });
  return response.session;
}

export function generateSessionId() {
  if (typeof globalThis.crypto?.randomUUID === "function") {
    return globalThis.crypto.randomUUID();
  }
  return `${Date.now()}-${Math.random().toString(36).slice(2)}`;
}

export function createInitialTurnRequest(
  sessionId: string,
  userId: string,
  text: string,
  modelSelection: ModelSelection | undefined,
  project: ProjectView | undefined,
  skills: SkillSuggestion[] = [],
  fileIds: string[] = [],
): StartTurnRequest {
  const input: TalosInputItem[] = [
    ...(text ? [{ type: "text" as const, text }] : []),
    ...(fileIds.length > 0 ? [{ type: "files" as const, fileIds }] : []),
    ...skills.map((skill) => ({
      type: "skill_selection" as const,
      name: skill.name,
      path: skill.path,
    })),
  ];
  return {
    session_id: sessionId,
    input,
    context: createContext(userId),
    modelSelection,
    project_id: project && !isUnboundProject(project.id) ? project.id : undefined,
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

export async function updateSessionProject(sessionId: string, project?: ProjectView) {
  const request: SessionPatchRequest = project && !isUnboundProject(project.id)
    ? {
        session_id: sessionId,
        conf: null,
        project_id: {
          type: "set",
          project_id: project.id,
        },
        current_directory: null,
      }
    : {
        session_id: sessionId,
        conf: null,
        project_id: {
          type: "clear",
        },
        current_directory: null,
      };
  await getTalosClient().sessions.patch(request);
}

export async function updateSessionModel(sessionId: string, modelSelection: ModelSelection) {
  const request: SessionPatchRequest = {
    session_id: sessionId,
    conf: {
      modelSelection,
    },
    project_id: null,
    current_directory: null,
  };
  await getTalosClient().sessions.patch(request);
}

export async function closeSession(sessionId: string) {
  await getTalosClient().sessions.close({ session_id: sessionId });
}

export async function deleteSession(sessionId: string) {
  await getTalosClient().sessions.delete({ session_id: sessionId });
}

export async function cancelActiveTurn(sessionId: string) {
  await getTalosClient().sessions.cancel({ session_id: sessionId });
}

export async function compactSession(sessionId: string) {
  const response = await getTalosClient().sessions.compact({ session_id: sessionId });
  return response.session;
}

export async function clearSession(sessionId: string) {
  const response = await getTalosClient().sessions.clear({ session_id: sessionId });
  return response.session;
}

export async function getActiveTurn(sessionId: string) {
  const response = await getTalosClient().sessions.activeTurn({ session_id: sessionId });
  return response.active_turn;
}

export async function getSessionHistory(sessionId: string) {
  const response = await getTalosClient().sessions.history({ session_id: sessionId });
  return response.history;
}

export async function recoverTimelineLiveTail(sessionId: string, afterEventId?: string) {
  const response = await getTalosClient().sessions.recoverTimeline({
    session_id: sessionId,
    after_event_id: afterEventId ?? null,
    replay_limit: null,
  });
  return response.snapshot;
}

export async function startTimelineTurn(
  request: StartTurnRequest,
  onAcceptedMessageId?: (messageId: string) => void,
): Promise<string> {
  const client = getTalosClient();
  const ack = await client.turns.start(request);
  if (!ack.message_id) {
    throw new Error(`Message ack did not include message_id: ${ack.turn_id}`);
  }
  onAcceptedMessageId?.(ack.message_id);
  return ack.message_id;
}

export async function retryTimelineTurn(
  request: RetryTurnRequest,
  onAcceptedMessageId?: (messageId: string) => void,
): Promise<string> {
  const client = getTalosClient();
  const ack = await client.turns.retry(request);
  if (!ack.message_id) {
    throw new Error(`Retry ack did not include message_id: ${ack.turn_id}`);
  }
  onAcceptedMessageId?.(ack.message_id);
  return ack.message_id;
}

export async function subscribeTimelineEvents(
  sessionId: string,
  onEvent: (event: TimelineEventEnvelope) => void,
  signal?: AbortSignal,
  afterEventId?: string,
) {
  return getTalosClient().turns.events(
    sessionId,
    afterEventId,
    onEvent,
    signal,
    { persistent: true },
  );
}

export function belongsToProject(session: SessionInfo, project?: ProjectView) {
  if (!project || isUnboundProject(project.id)) {
    return !session.project_id;
  }
  return session.project_id === project.id;
}

export type ChatApiModel = ModelInfo;
export type ChatApiSession = SessionInfo;
export type ChatApiActiveTurn = Awaited<ReturnType<typeof getActiveTurn>>;
export type ChatApiSessionHistory = TalosSessionHistorySnapshot;
export type ChatApiSessionDetail = SessionInfo;
