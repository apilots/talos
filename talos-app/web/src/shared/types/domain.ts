import { i18n } from "@/shared/i18n/setup";
import type { TalosContentItem, TalosMessageResponse, TalosOutputItem, TalosSession } from "./protocol";
import type {
  ApprovalMode,
  ChatRenderBlock,
  FileRecord,
  MemoryRecord,
  ProjectRecord,
  SessionUiStatus,
  StreamSessionState,
} from "./ui";

export const UNBOUND_PROJECT_ID = "__unbound_project__";

export function getEffectiveSessionStatus(
  transientStatus?: SessionUiStatus,
  streamState?: StreamSessionState,
): SessionUiStatus {
  if (streamState?.pendingApproval) {
    return "waiting_approval";
  }

  if (streamState?.error || transientStatus === "error") {
    return "error";
  }

  if (
    streamState?.isStreaming ||
    streamState?.connectionState === "awaiting" ||
    streamState?.connectionState === "detached" ||
    streamState?.connectionState === "reconnecting" ||
    streamState?.connectionState === "cancelling"
  ) {
    return "running";
  }

  return "done";
}

export function normalizeProjectDirectory(directory?: string | null) {
  return directory?.trim() || null;
}

export function createDefaultProjects(): ProjectRecord[] {
  const now = Date.now();
  return [
    {
      id: UNBOUND_PROJECT_ID,
      name: i18n.t("common:noProject"),
      directory: null,
      createdAt: now,
      updatedAt: now,
      origin: "default",
      sessionCount: 0,
    },
  ];
}

export function describeProjectDirectory(directory?: string | null) {
  return normalizeProjectDirectory(directory) ?? i18n.t("common:noProject");
}

export function getProjectRecordKey(project: ProjectRecord) {
  return project.id;
}

export function resolveProjectRecord(projects: ProjectRecord[], projectId?: string | null) {
  return projects.find((project) => project.id === projectId) ?? projects[0];
}

export function isUnboundProject(projectId?: string | null) {
  return !projectId || projectId === UNBOUND_PROJECT_ID;
}

export function getApprovalModeLabel(mode: ApprovalMode) {
  return mode === "skip" ? i18n.t("chat:fullAccess") : i18n.t("chat:defaultPermissions");
}

export function getApprovalModeHint(mode: ApprovalMode) {
  return mode === "skip"
    ? i18n.t("settings:fields.skipHint")
    : i18n.t("settings:fields.needsApprovalHint");
}

export function approvalModeToPolicy(mode: ApprovalMode) {
  return mode === "skip" ? "never" : "always";
}

export function getSessionApprovalMode(session?: TalosSession | null): ApprovalMode {
  const policy = session?.conf?.tools?.approval_policy;
  return policy === "always" ? "needs_approval" : "skip";
}

export function createDefaultMemory(): MemoryRecord[] {
  const now = Date.now();
  return [
    {
      id: "memory-1",
      title: "Role and background",
      detail: "You are a Rust backend engineer building Talos and care about strong API contracts.",
      tags: ["profile", "engineer"],
      updatedAt: now,
    },
    {
      id: "memory-2",
      title: "Code review preference",
      detail: "Prefer concise findings first, then risks, then next steps.",
      tags: ["review", "ui"],
      updatedAt: now - 60_000,
    },
    {
      id: "memory-3",
      title: "Workspace convention",
      detail: "New frontend work lives under talos-app/ and mirrors shared APIs first.",
      tags: ["workspace"],
      updatedAt: now - 120_000,
    },
    {
      id: "memory-4",
      title: "Communication preference",
      detail: "Prefer direct explanations in Chinese during collaboration and English in code artifacts.",
      tags: ["communication", "style"],
      updatedAt: now - 180_000,
    },
  ];
}

export function createDefaultFiles(): FileRecord[] {
  const now = Date.now();
  return [
    {
      id: "file-1",
      label: "web-ui-design.md",
      path: "spec/web-ui-design.md",
      summary: "Primary product and interaction design.",
      updatedAt: now,
    },
    {
      id: "file-2",
      label: "app-shell.tsx",
      path: "talos-app/web/src/widgets/app-layout/AppLayout.tsx",
      summary: "Shared application shell and fixed sidebar layout.",
      updatedAt: now - 60_000,
    },
    {
      id: "file-3",
      label: "web-ui-frontend-arch.md",
      path: "spec/web-ui-frontend-arch.md",
      summary: "Frontend layering and engineering rules.",
      updatedAt: now - 120_000,
    },
    {
      id: "file-4",
      label: "metrics.csv",
      path: "exports/metrics.csv",
      summary: "Sample operational export for dashboard review.",
      updatedAt: now - 180_000,
    },
  ];
}

export function outputContentToText(content: TalosContentItem[]) {
  return content
    .map((item) => {
      if (item.type === "text") {
        return item.text;
      }
      return JSON.stringify(item.value, null, 2);
    })
    .join("\n");
}

export function mapOutputItemToBlocks(item: TalosOutputItem, index: number): ChatRenderBlock[] {
  switch (item.type) {
    case "user_input":
      return [{ id: `history-user-${index}`, kind: "user", text: item.text }];
    case "output_text":
      return [{ id: `history-assistant-${index}`, kind: "assistant", text: item.text }];
    case "reasoning":
      return [{ id: `history-thinking-${index}`, kind: "thinking", text: item.text }];
    case "execution_summary":
      return [{ id: `history-summary-${index}`, kind: "system", text: item.text }];
    case "tool_call":
      return [
        {
          id: `history-tool-call-${item.call_id}`,
          kind: "tool",
          callId: item.call_id,
          toolName: item.name,
          arguments: item.arguments,
          status: "queued",
          progress: [],
        },
      ];
    case "tool_result":
      return [
        {
          id: `history-tool-result-${item.call_id}`,
          kind: "tool",
          callId: item.call_id,
          toolName: item.tool_name,
          arguments: null,
          status: item.is_error ? "error" : "done",
          output: outputContentToText(item.content),
          progress: [],
        },
      ];
  }
}

export function mapTurnsToBlocks(turns: TalosMessageResponse[]) {
  const blocks: ChatRenderBlock[] = [];

  turns.forEach((turn, turnIndex) => {
    turn.output.forEach((item, itemIndex) => {
      const blockIndex = turnIndex * 1000 + itemIndex;
      if (item.type === "tool_call") {
        blocks.push({
          id: `history-tool-${item.call_id}`,
          kind: "tool",
          callId: item.call_id,
          toolName: item.name,
          arguments: item.arguments,
          status: "queued",
          progress: [],
        });
        return;
      }

      if (item.type === "tool_result") {
        const existing = [...blocks]
          .reverse()
          .find((block): block is Extract<ChatRenderBlock, { kind: "tool" }> =>
            block.kind === "tool" && block.callId === item.call_id,
          );
        if (existing) {
          existing.toolName = item.tool_name;
          existing.output = outputContentToText(item.content);
          existing.status = item.is_error ? "error" : "done";
          return;
        }
      }

      blocks.push(...mapOutputItemToBlocks(item, blockIndex));
    });
  });

  return blocks;
}

export function getSessionModel(session: TalosSession) {
  return (
    session.config?.llm?.model ??
    session.config?.agent?.model ??
    "unknown"
  );
}
