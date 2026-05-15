import { TalosHttpError } from "@talos/sdk-browser";
import type {
  ModelInfo,
  ProjectRecord as ApiProjectRecord,
  SessionQuery,
  SessionUpdate,
  TalosContext,
  TalosMessageRequest,
  TalosMessageRetryRequest,
  TalosSession,
  TalosSessionHistorySnapshot,
  TimelineEventEnvelope,
  ToolApprovalRequest,
} from "@talos/sdk-browser";

import { getTalosClient } from "@/shared/api/client";
import { approvalModeToPolicy, isUnboundProject } from "@/shared/types/domain";
import type { ProjectRecord } from "@/shared/types/ui";
import type { ApprovalMode } from "@/shared/types/ui";

function createContext(userId: string): TalosContext {
  const randomPart = Math.random().toString(36).slice(2, 10);
  return {
    source_type: "http",
    provider: "sdk",
    user_id: userId,
    message_id: `msg_${randomPart}`,
    trace_id: `trace_${randomPart}`,
    channel_id: "web",
  };
}

function mapProjectRecord(project: ApiProjectRecord): ProjectRecord {
  return {
    id: project.project_id,
    name: project.name,
    directory: project.directory,
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
  const projects = await getTalosClient().projects.list();
  return projects.map(mapProjectRecord);
}

export async function listSessions() {
  return getTalosClient().sessions.list();
}

export async function listAdminSessions(query?: SessionQuery) {
  return getTalosClient().admin.sessions.list(query);
}

export async function deleteAdminSession(sessionId: string) {
  await getTalosClient().admin.sessions.delete(sessionId);
}

export async function listSessionsByProject(projectId: string) {
  return getTalosClient().sessions.list({ project_id: projectId });
}

export async function getSession(sessionId: string) {
  return getTalosClient().sessions.get(sessionId);
}

export async function createSession(
  userId: string,
  model: string | undefined,
  project: ProjectRecord | undefined,
  approvalMode: ApprovalMode,
) {
  return getTalosClient().sessions.create({
    context: createContext(userId),
    model,
    conf: {
      tools: {
        approval_policy: approvalModeToPolicy(approvalMode),
      },
    },
    project_id: project && !isUnboundProject(project.id) ? project.id : undefined,
  });
}

export async function createProject(input: string | CreateProjectInput) {
  const directory = typeof input === "string" ? input : input.directory;
  const name = typeof input === "string" ? deriveProjectName(directory) : input.name?.trim() || deriveProjectName(directory);
  const record = await getTalosClient().projects.create({
    name,
    directory,
  });
  return mapProjectRecord(record);
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

export async function updateSessionProject(sessionId: string, project?: ProjectRecord) {
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

export async function updateSessionApprovalMode(sessionId: string, approvalMode: ApprovalMode) {
  const patch: SessionUpdate = {
    conf: {
      tools: {
        approval_policy: approvalModeToPolicy(approvalMode),
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
  return getTalosClient().sessions.compact(sessionId);
}

export async function clearSession(sessionId: string) {
  return getTalosClient().sessions.clear(sessionId);
}

export async function getActiveTurn(sessionId: string) {
  return getTalosClient().sessions.activeTurn(sessionId);
}

export async function getSessionHistory(sessionId: string) {
  return getTalosClient().sessions.history(sessionId);
}

export async function recoverSessionLiveTail(sessionId: string, afterEventId?: string) {
  return getTalosClient().sessions.recover(sessionId, afterEventId);
}

export async function sendMessage(sessionId: string, text: string) {
  const request: TalosMessageRequest = {
    input: [{ type: "text", text }],
  };
  return getTalosClient().messages.send(sessionId, request);
}

export async function sendTimelineEvents(
  sessionId: string,
  text: string,
  onEvent: (event: TimelineEventEnvelope) => void,
  signal?: AbortSignal,
  onAcceptedMessageId?: (messageId: string) => void,
) {
  const request: TalosMessageRequest = {
    input: [{ type: "text", text }],
  };
  const client = getTalosClient();
  const ack = await client.messages.send(sessionId, request);
  if (!ack.message_id) {
    throw new Error(`Message ack did not include message_id: ${ack.turn_id}`);
  }
  onAcceptedMessageId?.(ack.message_id);
  return client.messages.eventsForMessage(sessionId, ack.message_id, onEvent, signal);
}

export async function retryTimelineEvents(
  sessionId: string,
  request: TalosMessageRetryRequest,
  onEvent: (event: TimelineEventEnvelope) => void,
  signal?: AbortSignal,
  onAcceptedMessageId?: (messageId: string) => void,
) {
  const client = getTalosClient();
  const ack = await client.messages.retry(sessionId, request);
  if (!ack.message_id) {
    throw new Error(`Retry ack did not include message_id: ${ack.turn_id}`);
  }
  onAcceptedMessageId?.(ack.message_id);
  return client.messages.eventsForMessage(sessionId, ack.message_id, onEvent, signal);
}

export async function subscribeTimelineEventsForMessage(
  sessionId: string,
  messageId: string,
  onEvent: (event: TimelineEventEnvelope) => void,
  signal?: AbortSignal,
  afterEventId?: string,
) {
  return getTalosClient().messages.eventsForMessage(
    sessionId,
    messageId,
    onEvent,
    signal,
    afterEventId,
  );
}

export async function submitToolApproval(
  sessionId: string,
  approval: ToolApprovalRequest,
) {
  await getTalosClient().toolApprovals.submit(sessionId, approval);
}

export function belongsToProject(session: TalosSession, project?: ProjectRecord) {
  if (!project || isUnboundProject(project.id)) {
    return !session.project_id;
  }
  return session.project_id === project.id;
}

export type ChatApiModel = ModelInfo;
export type ChatApiSession = TalosSession;
export type ChatApiActiveTurn = Awaited<ReturnType<typeof getActiveTurn>>;
export type ChatApiSessionHistory = TalosSessionHistorySnapshot;
export type ChatApiSessionDetail = TalosSession;
