import { TalosHttpError } from "@talos/sdk-browser";
import type {
  ModelInfo,
  ProjectInfo as ApiProjectInfo,
  RetryTurnRequest,
  SkillSuggestion,
  StartTurnRequest,
  SessionInfo,
  SessionUpdate,
  TalosContext,
  TalosInputItem,
  TalosSessionHistorySnapshot,
  TimelineEventEnvelope,
} from "@talos/sdk-browser";

import { getTalosClient } from "@/shared/api/client";
import { isUnboundProject } from "@/shared/types/domain";
import type { ProjectView } from "@/shared/types/ui";

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
  const response = await getTalosClient().models.list();
  return response.models;
}

export async function listProjects() {
  const response = await getTalosClient().projects.list();
  return response.projects.map(mapProjectView);
}

export async function listSessions() {
  const response = await getTalosClient().sessions.list();
  return response.sessions;
}

export async function listSessionsByProject(projectId: string) {
  const response = await getTalosClient().sessions.list({
    project_id: projectId,
    limit: null,
    offset: null,
  });
  return response.sessions;
}

export async function getSession(sessionId: string) {
  const response = await getTalosClient().sessions.get(sessionId);
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
  model: string | undefined,
  project: ProjectView | undefined,
  skills: SkillSuggestion[] = [],
): StartTurnRequest {
  const input: TalosInputItem[] = [
    { type: "text", text },
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
    model,
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
  const response = await getTalosClient().fs.directory.read({ path });
  return response.directory;
}

export async function createFsDirectory(parent: string, name: string) {
  return getTalosClient().fs.directory.create({ parent, name });
}

export async function deleteProject(projectId: string) {
  try {
    await getTalosClient().projects.delete(projectId);
  } catch (error) {
    if (error instanceof TalosHttpError && error.status === 409) {
      throw new Error("PROJECT_DELETE_CONFLICT");
    }
    throw error;
  }
}

export async function updateSessionProject(sessionId: string, project?: ProjectView) {
  const patch: SessionUpdate = project && !isUnboundProject(project.id)
    ? {
        project_id: {
          type: "set",
          project_id: project.id,
        },
      }
    : {
        project_id: {
          type: "clear",
        },
      };
  await getTalosClient().sessions.patch(sessionId, patch);
}

export async function updateSessionModel(sessionId: string, model: string) {
  const patch: SessionUpdate = {
    conf: {
      agent: {
        model,
      },
    },
  };
  await getTalosClient().sessions.patch(sessionId, patch);
}

export async function closeSession(sessionId: string) {
  await getTalosClient().sessions.close(sessionId);
}

export async function deleteSession(sessionId: string) {
  await getTalosClient().sessions.delete(sessionId);
}

export async function cancelActiveTurn(sessionId: string) {
  await getTalosClient().sessions.cancel(sessionId);
}

export async function compactSession(sessionId: string) {
  const response = await getTalosClient().sessions.compact(sessionId);
  return response.session;
}

export async function clearSession(sessionId: string) {
  const response = await getTalosClient().sessions.clear(sessionId);
  return response.session;
}

export async function getActiveTurn(sessionId: string) {
  const response = await getTalosClient().sessions.activeTurn(sessionId);
  return response.active_turn;
}

export async function getSessionHistory(sessionId: string) {
  const response = await getTalosClient().sessions.history(sessionId);
  return response.history;
}

export async function recoverSessionLiveTail(sessionId: string, afterEventId?: string) {
  const response = await getTalosClient().sessions.recover(sessionId, afterEventId);
  return response.snapshot;
}

export async function sendTimelineEvents(
  request: StartTurnRequest,
  onEvent: (event: TimelineEventEnvelope) => void,
  signal?: AbortSignal,
  onAcceptedMessageId?: (messageId: string) => void,
) {
  const client = getTalosClient();
  const ack = await client.turns.start(request);
  if (!ack.message_id) {
    throw new Error(`Message ack did not include message_id: ${ack.turn_id}`);
  }
  onAcceptedMessageId?.(ack.message_id);
  return client.turns.eventsForMessage(request.session_id, ack.message_id, onEvent, signal);
}

export async function retryTimelineEvents(
  request: RetryTurnRequest,
  onEvent: (event: TimelineEventEnvelope) => void,
  signal?: AbortSignal,
  onAcceptedMessageId?: (messageId: string) => void,
) {
  const client = getTalosClient();
  const ack = await client.turns.retry(request);
  if (!ack.message_id) {
    throw new Error(`Retry ack did not include message_id: ${ack.turn_id}`);
  }
  onAcceptedMessageId?.(ack.message_id);
  return client.turns.eventsForMessage(request.session_id, ack.message_id, onEvent, signal);
}

export async function subscribeTimelineEventsForMessage(
  sessionId: string,
  messageId: string,
  onEvent: (event: TimelineEventEnvelope) => void,
  signal?: AbortSignal,
  afterEventId?: string,
) {
  return getTalosClient().turns.eventsForMessage(
    sessionId,
    messageId,
    onEvent,
    signal,
    afterEventId,
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
