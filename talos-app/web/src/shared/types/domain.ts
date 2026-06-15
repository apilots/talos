import { i18n } from "@/shared/i18n/setup";
import type { TalosContentItem, TalosSession } from "./protocol";
import type {
  FileRecord,
  MemoryRecord,
  ProjectRecord,
  SessionUiStatus,
  StreamSessionState,
} from "./ui";

export const UNBOUND_PROJECT_ID = "__unbound_project__";
export const DEFAULT_SESSION_TITLE = "Untitled Session";

export function getEffectiveSessionStatus(
  transientStatus?: SessionUiStatus,
  streamState?: StreamSessionState,
): SessionUiStatus {
  if (streamState?.pendingUserAction) {
    return "running";
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

export function hasRealSessionTitle(title?: string | null) {
  const normalized = title?.trim() ?? "";
  return normalized.length > 0 && normalized !== DEFAULT_SESSION_TITLE;
}

export function resolveProjectRecord(projects: ProjectRecord[], projectId?: string | null) {
  return projects.find((project) => project.id === projectId) ?? projects[0];
}

export function isUnboundProject(projectId?: string | null) {
  return !projectId || projectId === UNBOUND_PROJECT_ID;
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
      switch (item.type) {
        case "text":
          return item.text;
        case "json":
          return JSON.stringify(unwrapTalosJsonValue(item.value), null, 2);
        case "file_read":
          return item.content;
        case "diff":
          return `diff: ${item.files.length} file(s)`;
        case "search_results":
          return item.matches
            .map((match) => `${match.path}:${match.line}: ${match.text}`)
            .join("\n");
        case "bash_output":
          return [item.stdout, item.stderr].filter(Boolean).join("\n") || `${item.status}`;
        case "web_reference":
          return [item.title ?? item.url, item.title ? item.url : "", item.snippet ?? ""]
            .filter(Boolean)
            .join("\n");
        case "artifact_ref":
          return item.label ?? item.artifact_id;
        case "mcp_resource_ref":
          return `${item.server}:${item.uri}`;
        case "progress":
          return item.message;
        default:
          return assertNeverToolContentItem(item);
      }
    })
    .join("\n");
}

function unwrapTalosJsonValue(value: unknown) {
  if (
    value &&
    typeof value === "object" &&
    "value" in value &&
    Object.keys(value).length === 1
  ) {
    return (value as { value: unknown }).value;
  }
  return value;
}

function assertNeverToolContentItem(item: never): string {
  return JSON.stringify(item);
}

export function getSessionModel(session: TalosSession) {
  return (
    session.config?.llm?.model ??
    session.config?.agent?.model ??
    "unknown"
  );
}
