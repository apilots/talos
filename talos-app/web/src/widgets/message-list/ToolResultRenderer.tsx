import type { ReactNode } from "react";

import type { TalosContentItem } from "@/shared/types/protocol";
import type { ChatTimelineItem, ChatToolStatus } from "@/shared/types/ui";

type ToolProgress = Extract<ChatTimelineItem, { kind: "tool_call" }>["progress"];

type ToolResultRenderModel = {
  summary: string;
  preview: string[];
  body: ReactNode;
};

export function buildToolResultModel({
  contentItems,
  fallbackOutput,
  progress,
  status,
}: {
  contentItems?: TalosContentItem[];
  fallbackOutput?: string;
  progress: ToolProgress;
  status: ChatToolStatus;
}): ToolResultRenderModel {
  const items = contentItems ?? [];
  if (items.some((item) => item.type === "bash_output")) {
    return buildTaskModel(items, progress, status);
  }
  if (items.some((item) => item.type === "diff")) {
    return buildDiffModel(items);
  }
  if (items.some((item) => item.type === "file_read")) {
    return buildFileModel(items);
  }
  if (items.some((item) => item.type === "search_results")) {
    return buildExploredModel(items);
  }
  if (items.some((item) => item.type === "web_reference")) {
    return buildReferenceModel(items);
  }
  if (items.length > 0) {
    return buildTextJsonModel(items, status);
  }
  return buildProgressFallbackModel(progress, fallbackOutput, status);
}

export function ToolResultRenderer(props: {
  contentItems?: TalosContentItem[];
  fallbackOutput?: string;
  progress: ToolProgress;
  status: ChatToolStatus;
}) {
  const model = buildToolResultModel(props);
  return <div className="space-y-3">{model.body}</div>;
}

export function ToolResultPreview(props: {
  contentItems?: TalosContentItem[];
  fallbackOutput?: string;
  progress: ToolProgress;
  status: ChatToolStatus;
}) {
  const model = buildToolResultModel(props);
  if (model.preview.length === 0) {
    return null;
  }
  return (
    <div className="border-t border-border/55 px-4 pb-3 text-[12px] leading-5 text-muted">
      {model.preview.slice(0, 2).map((line, index) => (
        <div className="truncate" key={`${line}-${index}`}>
          {line}
        </div>
      ))}
    </div>
  );
}

export function ToolResultSummary(props: {
  contentItems?: TalosContentItem[];
  fallbackOutput?: string;
  progress: ToolProgress;
  status: ChatToolStatus;
}) {
  return buildToolResultModel(props).summary;
}

function buildTaskModel(
  items: TalosContentItem[],
  progress: ToolProgress,
  status: ChatToolStatus,
): ToolResultRenderModel {
  const taskItems = items.filter((item) => item.type === "bash_output");
  const stdout = taskItems.map((item) => item.stdout).filter(Boolean).join("\n");
  const stderr = taskItems.map((item) => item.stderr).filter(Boolean).join("\n");
  const liveOutput = progress
    .filter((entry) => entry.kind === "stdout" || entry.kind === "stderr")
    .map((entry) => entry.message)
    .join("");
  const exitCode = lastDefined(taskItems.map((item) => item.exit_code));
  const elapsedMs = lastDefined(taskItems.map((item) => item.elapsed_ms));
  const output = [stdout, stderr].filter(Boolean).join(stderr && stdout ? "\n\n" : "") || liveOutput;
  const firstLine = firstNonEmptyLine(output);
  const summary =
    status === "running"
      ? "Running"
      : status === "error"
        ? exitCode !== undefined && exitCode !== null
          ? `Error · exit ${exitCode}${formatDurationSuffix(elapsedMs)}`
          : `Error${formatDurationSuffix(elapsedMs)}`
        : exitCode !== undefined && exitCode !== null
          ? `Done · exit ${exitCode}${formatDurationSuffix(elapsedMs)}`
          : `Done${formatDurationSuffix(elapsedMs)}`;
  return {
    summary,
    preview: firstLine ? [firstLine] : [],
    body: (
      <pre className="max-h-80 overflow-auto rounded-lg border border-border/70 bg-surface/85 p-3 font-mono text-[12px] leading-5 text-ink">
        {output || summary}
      </pre>
    ),
  };
}

function buildDiffModel(items: TalosContentItem[]): ToolResultRenderModel {
  const diffItems = items.filter((item) => item.type === "diff");
  let files = 0;
  let added = 0;
  let removed = 0;
  const lines: string[] = [];
  for (const item of diffItems) {
    for (const file of item.files) {
      files += 1;
      lines.push(`diff -- ${file.path}`);
      if (file.old_path) {
        lines.push(`rename from ${file.old_path}`);
        lines.push(`rename to ${file.path}`);
      }
      for (const hunk of file.hunks) {
        lines.push(
          `@@ -${hunk.old_start ?? 0},${hunk.old_lines ?? 0} +${hunk.new_start ?? 0},${hunk.new_lines ?? 0} @@`,
        );
        for (const line of hunk.lines) {
          if (line.kind === "added") {
            added += 1;
            lines.push(`+${line.text}`);
          } else if (line.kind === "removed") {
            removed += 1;
            lines.push(`-${line.text}`);
          } else {
            lines.push(` ${line.text}`);
          }
        }
      }
      lines.push("");
    }
  }
  const summary = `Done · ${files} file(s) · +${added} -${removed}`;
  return {
    summary,
    preview: [`${files} file(s) changed · +${added} -${removed}`],
    body: (
      <pre className="max-h-96 overflow-auto rounded-lg border border-border/70 bg-surface/85 p-3 font-mono text-[12px] leading-5 text-ink">
        {lines.join("\n").trimEnd() || summary}
      </pre>
    ),
  };
}

function buildFileModel(items: TalosContentItem[]): ToolResultRenderModel {
  const fileItems = items.filter((item) => item.type === "file_read");
  const lines: string[] = [];
  let count = 0;
  for (const item of fileItems) {
    const start = item.offset + 1;
    lines.push(`${item.path}:${start}-${Math.min(item.offset + item.limit, item.total_lines)}`);
    for (const [index, line] of item.content.split("\n").entries()) {
      count += 1;
      lines.push(`${String(start + index).padStart(4, " ")} ${line}`);
    }
  }
  const summary = `Done · ${count} lines`;
  return {
    summary,
    preview: [firstNonEmptyLine(lines.join("\n")) ?? summary],
    body: (
      <pre className="max-h-96 overflow-auto rounded-lg border border-border/70 bg-surface/85 p-3 font-mono text-[12px] leading-5 text-ink">
        {lines.join("\n")}
      </pre>
    ),
  };
}

function buildExploredModel(items: TalosContentItem[]): ToolResultRenderModel {
  const lines: string[] = [];
  let count = 0;
  for (const item of items) {
    if (item.type === "search_results") {
      count += item.matches.length;
      for (const match of item.matches) {
        lines.push(`${match.path}:${match.line} ${match.text}`);
        for (const before of match.before.slice(0, 2)) {
          lines.push(`  ${before}`);
        }
        for (const after of match.after.slice(0, 2)) {
          lines.push(`  ${after}`);
        }
      }
    } else if (item.type === "web_reference") {
      count += 1;
      lines.push(item.title ?? item.url);
      lines.push(`  ${item.source_domain ?? item.url}`);
      if (item.snippet) {
        lines.push(`  ${item.snippet}`);
      }
    }
  }
  const summary = `Done · ${count} result(s)`;
  return {
    summary,
    preview: lines.filter(Boolean).slice(0, 2),
    body: <ResultList lines={lines} />,
  };
}

function buildReferenceModel(items: TalosContentItem[]): ToolResultRenderModel {
  const lines = items.flatMap((item) => {
    if (item.type === "web_reference") {
      return [item.title ?? item.url, item.source_domain ?? item.url, item.snippet ?? ""].filter(Boolean);
    }
    if (item.type === "artifact_ref") {
      return [`artifact: ${item.label ?? item.artifact_id}${item.mime_type ? ` · ${item.mime_type}` : ""}`];
    }
    if (item.type === "mcp_resource_ref") {
      return [`mcp: ${item.server}:${item.uri}${item.mime_type ? ` · ${item.mime_type}` : ""}`];
    }
    return [];
  });
  const summary = `Done · ${lines.length || 1} reference(s)`;
  return {
    summary,
    preview: lines.slice(0, 2),
    body: <ResultList lines={lines} />,
  };
}

function buildTextJsonModel(items: TalosContentItem[], status: ChatToolStatus): ToolResultRenderModel {
  const lines = items.flatMap((item) => {
    if (item.type === "text") {
      return item.text.split("\n");
    }
    if (item.type === "json") {
      return [JSON.stringify(unwrapTalosJsonValue(item.value), null, 2)];
    }
    if (item.type === "progress") {
      return [item.message];
    }
    return [];
  });
  const text = lines.join("\n");
  const summary = status === "running" ? "Running" : status === "error" ? "Error" : "Done";
  return {
    summary,
    preview: [firstNonEmptyLine(text) ?? summary],
    body: (
      <pre className="max-h-80 overflow-auto rounded-lg border border-border/70 bg-surface/85 p-3 font-mono text-[12px] leading-5 text-ink">
        {text}
      </pre>
    ),
  };
}

function buildProgressFallbackModel(
  progress: ToolProgress,
  fallbackOutput: string | undefined,
  status: ChatToolStatus,
): ToolResultRenderModel {
  const output =
    fallbackOutput ||
    progress
      .filter((entry) => entry.kind === "stdout" || entry.kind === "stderr")
      .map((entry) => entry.message)
      .join("");
  const summary = status === "running" ? "Running" : status === "error" ? "Error" : "Done";
  return {
    summary,
    preview: output ? [firstNonEmptyLine(output) ?? output] : [],
    body: output ? (
      <pre className="max-h-80 overflow-auto rounded-lg border border-border/70 bg-surface/85 p-3 font-mono text-[12px] leading-5 text-ink">
        {output}
      </pre>
    ) : (
      <div className="text-[12px] text-muted">{summary}</div>
    ),
  };
}

function ResultList({ lines }: { lines: string[] }) {
  return (
    <div className="space-y-1 rounded-lg border border-border/70 bg-surface/85 p-3 text-[12px] leading-5 text-ink">
      {lines.filter(Boolean).map((line, index) => (
        <div className={line.startsWith("  ") ? "pl-3 text-muted" : "font-medium"} key={`${line}-${index}`}>
          {line.trimStart()}
        </div>
      ))}
    </div>
  );
}

function firstNonEmptyLine(text: string) {
  return text.split("\n").find((line) => line.trim().length > 0)?.trim();
}

function lastDefined<T>(values: Array<T | null | undefined>) {
  for (let index = values.length - 1; index >= 0; index -= 1) {
    const value = values[index];
    if (value !== undefined && value !== null) {
      return value;
    }
  }
  return undefined;
}

function formatDurationSuffix(value: bigint | number | null | undefined) {
  if (value === undefined || value === null) {
    return "";
  }
  return ` · ${(Number(value) / 1000).toFixed(1)}s`;
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
