import type {
  ModelInfo,
  SessionUpdate,
  TalosContext,
  TalosEvent,
  TalosMessageRequest,
  TalosMessageResponse,
  TalosSession,
  ToolApprovalRequest,
} from "@talos/sdk";

import { getTalosClient } from "@/shared/api/client";
import type { ProjectRecord } from "@/shared/types/domain";
import { resolveProjectRootPath } from "@/shared/types/domain";

function createContext(userId: string, project?: ProjectRecord): TalosContext {
  const randomPart = Math.random().toString(36).slice(2, 10);
  const projectRoot = project ? resolveProjectRootPath(project.root) : undefined;
  return {
    source_type: "http",
    provider: "sdk",
    user_id: userId,
    message_id: `msg_${randomPart}`,
    trace_id: `trace_${randomPart}`,
    channel_id: "web",
    project_root: projectRoot,
  };
}

export async function listModels() {
  return getTalosClient().models.list();
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
    context: createContext(userId, project),
    model,
  });
}

export async function updateSessionProject(sessionId: string, project: ProjectRecord) {
  const resolvedPath = resolveProjectRootPath(project.root);
  const patch: SessionUpdate = resolvedPath
    ? {
        conf: {},
        project_root: {
          type: "set",
          path: resolvedPath,
        },
      }
    : {
        conf: {},
        project_root: {
          type: "clear",
        },
      };
  await getTalosClient().sessions.patch(sessionId, patch);
}

export async function updateSessionModel(sessionId: string, model: string) {
  const patch: SessionUpdate = {
    conf: {
      "agent.model": model,
    },
  };
  await getTalosClient().sessions.patch(sessionId, patch);
}

export async function closeSession(sessionId: string) {
  await getTalosClient().sessions.close(sessionId);
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
  if (!project) {
    return true;
  }
  const path = resolveProjectRootPath(project.root);
  const sessionPath = session.context.project_root ?? __TALOS_DEFAULT_PROJECT_ROOT__;
  return sessionPath === path;
}

export type ChatApiModel = ModelInfo;
export type ChatApiSession = TalosSession;
export type ChatApiSessionDetail = TalosSession;
export type ChatApiTurns = TalosMessageResponse[];
