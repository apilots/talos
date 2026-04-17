import type { TalosToolProgressKind } from "./protocol";

export type RuntimeMode = "cloud" | "local";
export type ApprovalMode = "needs_approval" | "skip";
export type ThreadFilter = "all" | "running" | "waiting_approval" | "done" | "error";
export type SessionUiStatus = "running" | "waiting_approval" | "done" | "error";

export type ProjectRoot =
  | { type: "managed_workspace"; name: string }
  | { type: "server_visible_path"; path: string }
  | { type: "desktop_local_path"; path: string };

export interface ProjectRecord {
  id: string;
  name: string;
  root: ProjectRoot;
  createdAt: number;
  lastUsedAt: number;
  origin?: "default" | "inferred" | "manual";
  sessionCount?: number;
}

export interface MemoryRecord {
  id: string;
  title: string;
  detail: string;
  tags: string[];
  updatedAt: number;
}

export interface FileRecord {
  id: string;
  label: string;
  path: string;
  summary: string;
  updatedAt: number;
}

export interface AdminMetric {
  id: string;
  label: string;
  value: string;
  trend: string;
}

export type ChatToolStatus =
  | "queued"
  | "running"
  | "waiting_approval"
  | "done"
  | "error";

export type StreamConnectionState =
  | "idle"
  | "awaiting"
  | "streaming"
  | "detached"
  | "reconnecting"
  | "cancelling";

export type ChatRenderBlock =
  | {
      id: string;
      kind: "user" | "assistant" | "thinking" | "system";
      text: string;
      live?: boolean;
    }
  | {
      id: string;
      kind: "status";
      variant: "thinking" | "reconnecting" | "cancelling";
      title: string;
      detail: string;
      live?: boolean;
    }
  | {
      id: string;
      kind: "tool";
      callId: string;
      toolName: string;
      arguments: unknown;
      status: ChatToolStatus;
      output?: string;
      progress: Array<{ kind: TalosToolProgressKind; message: string }>;
      prompt?: string;
      timingMs?: number | null;
      live?: boolean;
    };

export interface PendingApproval {
  callId: string;
  toolName: string;
  prompt: string;
  arguments: unknown;
}

export interface StreamSessionState {
  isStreaming: boolean;
  connectionState?: StreamConnectionState;
  lastSequence?: number;
  activeMessageId?: string;
  expectedMessageCount?: number;
  blocks: ChatRenderBlock[];
  pendingApproval?: PendingApproval;
  error?: string;
}

export interface InferredProjectsResult {
  projects: ProjectRecord[];
  preferredProjectId: string;
}
