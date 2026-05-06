import type { TalosPendingUserAction, TalosToolProgressKind } from "./protocol";

export type RuntimeMode = "cloud" | "local";
export type ApprovalMode = "needs_approval" | "skip";
export type ReasoningLevel = "low" | "medium" | "high" | "extra_high";
export type ThreadFilter = "all" | "running" | "waiting_approval" | "done" | "error";
export type SessionUiStatus = "running" | "waiting_approval" | "done" | "error";

export interface ProjectRecord {
  id: string;
  name: string;
  directory?: string | null;
  createdAt: number;
  updatedAt: number;
  origin?: "default" | "server";
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

export interface PendingInput {
  text: string;
  clientMessageId?: string;
  serverMessageId?: string;
}

export interface StreamSessionState {
  isStreaming: boolean;
  connectionState?: StreamConnectionState;
  lastSequence?: number;
  activeMessageId?: string;
  expectedMessageCount?: number;
  reconnectAttempt?: number;
  showReconnectControls?: boolean;
  activeInputText?: string;
  activeClientMessageId?: string;
  blocks: ChatRenderBlock[];
  pendingInput?: PendingInput;
  pendingUserAction?: TalosPendingUserAction;
  pendingApproval?: PendingApproval;
  error?: string;
}
