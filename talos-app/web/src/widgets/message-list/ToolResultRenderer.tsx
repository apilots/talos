import type { ReactNode } from "react";

import { jsonContentValue } from "@talos/sdk-browser";
import { cn } from "@/shared/lib/cn";
import type { TalosContentItem } from "@/shared/types/protocol";
import type { ChatTimelineItem, ChatToolStatus } from "@/shared/types/ui";

type ToolProgress = Extract<ChatTimelineItem, { kind: "tool_call" }>["progress"];
type SubagentResultItem = Extract<TalosContentItem, { type: "subagent_result" }>;
type SubagentResult = SubagentResultItem["result"];
type SubagentSnapshot = Extract<
  SubagentResult,
  { operation: "waitAgent" | "interruptAgent" | "followupTask" }
>["result"];
type AgentTreeNode = Extract<SubagentResult, { operation: "listAgents" }>["result"]["children"][number];

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
  const subagentItem = items.find(
    (item): item is SubagentResultItem => item.type === "subagent_result",
  );
  if (subagentItem) {
    return buildSubagentModel(subagentItem.result, status);
  }
  if (items.some((item) => item.type === "bash_output")) {
    return buildTaskModel(items, progress, status);
  }
  if (items.some((item) => item.type === "diff")) {
    return buildDiffModel(items, status);
  }
  if (items.some((item) => item.type === "file_read")) {
    return buildFileModel(items, status);
  }
  if (items.some((item) => item.type === "search_results")) {
    return buildExploredModel(items, status);
  }
  if (items.some((item) => item.type === "web_reference")) {
    return buildReferenceModel(items, status);
  }
  if (items.length > 0) {
    return buildTextJsonModel(items, status);
  }
  return buildProgressFallbackModel(progress, fallbackOutput, status);
}

function buildSubagentModel(
  result: SubagentResult,
  status: ChatToolStatus,
): ToolResultRenderModel {
  const lines: string[] = [];
  let summary: string;
  switch (result.operation) {
    case "spawnAgent":
      summary = `Spawned ${result.result.subagent.path}`;
      lines.push(
        `agent: ${result.result.subagent.path}`,
        `agent id: ${result.result.agentId}`,
        `session: ${result.result.subagent.sessionId}`,
        `turn: ${result.result.turn.turnId}`,
      );
      break;
    case "sendMessage":
      summary = `Message accepted by ${result.result.subagent.path}`;
      lines.push(
        `agent: ${result.result.subagent.path}`,
        `agent id: ${result.result.agentId}`,
        `session: ${result.result.subagent.sessionId}`,
        `turn: ${result.result.turn.turnId}`,
      );
      break;
    case "waitAgent":
    case "interruptAgent":
    case "followupTask":
      summary = `${result.result.subagent.path} · ${result.result.outcome.status}`;
      pushSubagentSnapshot(lines, result.result);
      break;
    case "listAgents":
      for (const child of result.result.children) {
        pushAgentTreeNode(lines, child, 0);
      }
      summary = `${lines.length} agent(s)`;
      break;
  }
  return {
    summary: `${toolResultStateLabel(status)} · ${summary}`,
    preview: lines.slice(0, 2),
    body: <ResultList lines={lines} />,
  };
}

function pushSubagentSnapshot(lines: string[], snapshot: SubagentSnapshot) {
  lines.push(
    `agent: ${snapshot.agentId}`,
    `session: ${snapshot.subagent.sessionId}`,
    `turn: ${snapshot.outcome.turnId}`,
  );
  if (snapshot.outcome.status === "completed" && snapshot.outcome.output) {
    lines.push(formatBoundedText(snapshot.outcome.output, snapshot.outcome.truncated));
  }
  if (snapshot.outcome.status === "failed") {
    lines.push(formatBoundedText(snapshot.outcome.error, snapshot.outcome.truncated));
  }
  if (snapshot.latestUpdate?.text) {
    lines.push(formatBoundedText(snapshot.latestUpdate.text, snapshot.latestUpdate.truncated));
  }
}

function pushAgentTreeNode(lines: string[], node: AgentTreeNode, depth: number) {
  lines.push(
    `${"  ".repeat(depth)}${node.snapshot.subagent.path} · ${node.snapshot.outcome.status}`,
  );
  for (const child of node.children) {
    pushAgentTreeNode(lines, child, depth + 1);
  }
}

function formatBoundedText(text: string, truncated: boolean) {
  return truncated ? `${text} [truncated]` : text;
}

export function ToolResultRenderer(props: {
  contentItems?: TalosContentItem[];
  compact?: boolean;
  fallbackOutput?: string;
  progress: ToolProgress;
  status: ChatToolStatus;
}) {
  const model = buildToolResultModel(props);
  const summary = props.compact
    ? model.summary.replace(/^(Queued|Running|Waiting for input|Completed|Failed)(?: · )?/, "")
    : model.summary;
  return (
    <div
      className={cn(
        "space-y-3",
        props.compact &&
          "[&_[data-tool-result-list=true]]:border-0 [&_[data-tool-result-list=true]]:bg-transparent [&_[data-tool-result-list=true]]:p-0",
      )}
      data-tool-result-renderer={props.compact ? "compact" : "default"}
    >
      {summary ? <ToolResultSummaryLine summary={summary} /> : null}
      {model.body}
    </div>
  );
}

export function ToolResultPreview(props: {
  contentItems?: TalosContentItem[];
  compact?: boolean;
  fallbackOutput?: string;
  progress: ToolProgress;
  status: ChatToolStatus;
}) {
  const model = buildToolResultModel(props);
  const summary = props.compact
    ? model.summary.replace(/^(Queued|Running|Waiting for input|Completed|Failed)(?: · )?/, "")
    : model.summary;
  if (model.preview.length === 0 && !summary) {
    return null;
  }
  return (
    <div
      className={cn(
        "space-y-1 text-[12px] leading-5 text-muted",
        props.compact ? "px-1 pb-1 pt-0.5" : "border-t border-border/55 px-4 py-3",
      )}
      data-tool-result-preview={props.compact ? "compact" : "default"}
    >
      {summary ? <ToolResultSummaryLine summary={summary} /> : null}
      {model.preview.slice(0, 2).map((line, index) => (
        <div className="truncate" key={`${line}-${index}`}>
          {line}
        </div>
      ))}
    </div>
  );
}

function ToolResultSummaryLine({ summary }: { summary: string }) {
  return <div className="font-medium text-ink/75">{summary}</div>;
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
  const output = [stdout, stderr].filter(Boolean).join(stderr && stdout ? "\n\n" : "") || liveOutput;
  const firstLine = firstNonEmptyLine(output);
  const exitSuffix = exitCode !== undefined && exitCode !== null ? ` · exit ${exitCode}` : "";
  const summary = `${toolResultStateLabel(status)}${exitSuffix}`;
  return {
    summary,
    preview: firstLine ? [firstLine] : [],
    body: output ? (
      <pre className="max-h-80 overflow-auto rounded-lg border border-border/70 bg-surface/85 p-3 font-mono text-[12px] leading-5 text-ink">
        {output}
      </pre>
    ) : null,
  };
}

function buildDiffModel(items: TalosContentItem[], status: ChatToolStatus): ToolResultRenderModel {
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
  const summary = `${toolResultStateLabel(status)} · ${files} file(s) · +${added} -${removed}`;
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

function buildFileModel(items: TalosContentItem[], status: ChatToolStatus): ToolResultRenderModel {
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
  const summary = `${toolResultStateLabel(status)} · ${count} lines`;
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

function buildExploredModel(items: TalosContentItem[], status: ChatToolStatus): ToolResultRenderModel {
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
  const summary = `${toolResultStateLabel(status)} · ${count} result(s)`;
  return {
    summary,
    preview: lines.filter(Boolean).slice(0, 2),
    body: <ResultList lines={lines} />,
  };
}

function buildReferenceModel(items: TalosContentItem[], status: ChatToolStatus): ToolResultRenderModel {
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
  const summary = `${toolResultStateLabel(status)} · ${lines.length || 1} reference(s)`;
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
      return [JSON.stringify(jsonContentValue(item.value), null, 2)];
    }
    if (item.type === "progress") {
      return [item.message];
    }
    return [];
  });
  const text = lines.join("\n");
  const summary = toolResultStateLabel(status);
  return {
    summary,
    preview: [firstNonEmptyLine(text) ?? summary],
    body: text ? (
      <pre className="max-h-80 overflow-auto rounded-lg border border-border/70 bg-surface/85 p-3 font-mono text-[12px] leading-5 text-ink">
        {text}
      </pre>
    ) : null,
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
  const summary = toolResultStateLabel(status);
  return {
    summary,
    preview: output ? [firstNonEmptyLine(output) ?? output] : [],
    body: output ? (
      <pre className="max-h-80 overflow-auto rounded-lg border border-border/70 bg-surface/85 p-3 font-mono text-[12px] leading-5 text-ink">
        {output}
      </pre>
    ) : null,
  };
}

function toolResultStateLabel(status: ChatToolStatus) {
  switch (status) {
    case "queued":
      return "Queued";
    case "running":
      return "Running";
    case "waiting_dynamic_input":
      return "Waiting for input";
    case "done":
      return "Completed";
    case "error":
      return "Failed";
  }
}

function ResultList({ lines }: { lines: string[] }) {
  return (
    <div
      className="space-y-1 rounded-lg border border-border/70 bg-surface/85 p-3 text-[12px] leading-5 text-ink"
      data-tool-result-list="true"
    >
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
