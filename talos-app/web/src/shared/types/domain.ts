import { i18n } from "@/shared/i18n/setup";
import type { TalosContentItem, TalosMessageResponse, TalosOutputItem, TalosSession } from "./protocol";
import type {
  ApprovalMode,
  ChatRenderBlock,
  FileRecord,
  InferredProjectsResult,
  MemoryRecord,
  ProjectRecord,
  ProjectRoot,
  SessionUiStatus,
  StreamSessionState,
} from "./ui";

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

function normalizeWorkspacePath(path?: string | null) {
  if (!path) {
    return __TALOS_DEFAULT_PROJECT_ROOT__;
  }

  if (
    path === "~/.talos/workspace-talos" &&
    __TALOS_DEFAULT_PROJECT_ROOT__.endsWith("/.talos/workspace-talos")
  ) {
    return __TALOS_DEFAULT_PROJECT_ROOT__;
  }

  return path;
}

export function resolveProjectRootPath(root: ProjectRoot): string | undefined {
  switch (root.type) {
    case "managed_workspace":
      return __TALOS_DEFAULT_PROJECT_ROOT__;
    case "server_visible_path":
      return root.path;
    case "desktop_local_path":
      return root.path;
  }
}

export function describeProjectRoot(root: ProjectRoot) {
  return normalizeWorkspacePath(resolveProjectRootPath(root));
}

export function createDefaultProjects(): ProjectRecord[] {
  const now = Date.now();
  return [
    {
      id: "workspace-default",
      name: i18n.t("common:defaultWorkspace"),
      root: { type: "server_visible_path", path: __TALOS_DEFAULT_PROJECT_ROOT__ },
      createdAt: now,
      lastUsedAt: now,
      origin: "default",
      sessionCount: 0,
    },
  ];
}

function slugifyProjectPath(path: string) {
  return path
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 64);
}

export function getProjectRecordKey(root: ProjectRoot) {
  const resolvedPath = resolveProjectRootPath(root);
  if (resolvedPath === __TALOS_DEFAULT_PROJECT_ROOT__) {
    return "workspace-default";
  }

  if (root.type === "managed_workspace") {
    return "workspace-default";
  }

  return `workspace-path-${slugifyProjectPath(root.path)}`;
}

export function getProjectDisplayName(root: ProjectRoot) {
  const resolvedPath = normalizeWorkspacePath(resolveProjectRootPath(root));
  if (resolvedPath === __TALOS_DEFAULT_PROJECT_ROOT__) {
    return i18n.t("common:defaultWorkspace");
  }

  return resolvedPath.split("/").filter(Boolean).at(-1) || resolvedPath;
}

export function getApprovalModeLabel(mode: ApprovalMode) {
  return mode === "skip" ? i18n.t("chat:fullAccess") : i18n.t("chat:defaultPermissions");
}

export function getApprovalModeHint(mode: ApprovalMode) {
  return mode === "skip"
    ? i18n.t("settings:fields.skipHint")
    : i18n.t("settings:fields.needsApprovalHint");
}

export function inferProjectsFromSessions(sessions: TalosSession[]): InferredProjectsResult {
  const defaultProjects = createDefaultProjects();
  const inferred = new Map<string, ProjectRecord>();

  for (const session of sessions) {
    const root: ProjectRoot = {
      type: "server_visible_path",
      path: normalizeWorkspacePath(session.context.project_root),
    };
    const key = getProjectRecordKey(root);
    const existing = inferred.get(key);
    if (existing) {
      existing.sessionCount = (existing.sessionCount ?? 0) + 1;
      existing.lastUsedAt = Math.max(existing.lastUsedAt, session.state.last_activity);
      continue;
    }
    inferred.set(key, {
      id: key,
      name: getProjectDisplayName(root),
      root,
      createdAt: session.state.created_at,
      lastUsedAt: session.state.last_activity,
      origin: "inferred",
      sessionCount: 1,
    });
  }

  for (const project of defaultProjects) {
    const key = getProjectRecordKey(project.root);
    const existing = inferred.get(key);
    if (existing) {
      existing.name = project.name;
      existing.origin = "default";
    } else {
      inferred.set(key, project);
    }
  }

  const projects = [...inferred.values()].sort((left, right) => {
    const countDiff = (right.sessionCount ?? 0) - (left.sessionCount ?? 0);
    if (countDiff !== 0) {
      return countDiff;
    }
    return right.lastUsedAt - left.lastUsedAt;
  });
  return {
    projects,
    preferredProjectId: projects[0]?.id ?? defaultProjects[0].id,
  };
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

export function getSessionProjectLabel(session: TalosSession) {
  return normalizeWorkspacePath(session.context.project_root);
}
