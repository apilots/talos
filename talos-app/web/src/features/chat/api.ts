import type {
  ModelInfo,
  ProjectRecord as ApiProjectRecord,
  SessionUpdate,
  TalosContext,
  TalosEvent,
  TalosMessageRequest,
  TalosMessageResponse,
  TalosSession,
  ToolApprovalRequest,
} from "@talos/sdk";

import { getTalosClient } from "@/shared/api/client";
import { isUnboundProject } from "@/shared/types/domain";
import type { ProjectRecord } from "@/shared/types/ui";

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
  };
}

function deriveProjectName(directory: string) {
  const normalized = directory.trim().replace(/\/+$/, "");
  return normalized.split("/").filter(Boolean).at(-1) || "project";
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

export async function getSession(sessionId: string) {
  return getTalosClient().sessions.get(sessionId);
}

export async function getSessionTurns(sessionId: string) {
  return getTalosClient().sessions.turns(sessionId);
}

export async function createSession(userId: string, model: string | undefined, project?: ProjectRecord) {
  return getTalosClient().sessions.create({
    context: createContext(userId),
    model,
    project_id: project && !isUnboundProject(project.id) ? project.id : undefined,
  });
}

export async function createProject(directory: string) {
  const record = await getTalosClient().projects.create({
    name: deriveProjectName(directory),
    directory,
  });
  return mapProjectRecord(record);
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

export async function sendMessage(sessionId: string, text: string) {
  const request: TalosMessageRequest = {
    input: [{ type: "text", text }],
  };
  return getTalosClient().messages.send(sessionId, request);
}

export async function sendMessageStream(
  sessionId: string,
  text: string,
  onEvent: (event: TalosEvent) => void,
  signal?: AbortSignal,
) {
  const request: TalosMessageRequest = {
    input: [{ type: "text", text }],
  };
  return getTalosClient().messages.sendStream(sessionId, request, onEvent, signal);
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
export type ChatApiSessionDetail = TalosSession;
export type ChatApiTurns = TalosMessageResponse[];
