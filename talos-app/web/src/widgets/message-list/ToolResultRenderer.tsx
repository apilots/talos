import { FileImage } from "./FileImage";
import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";

import { jsonContentValue } from "@talos/sdk-browser";
import { decodeApiError } from "@talos/sdk-core";
import { apiErrorMessage } from "@/shared/i18n/apiErrorMessage";
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

type ToolFailureReason = "failed" | "timedOut" | "cancelled" | "captureFailed" | "unspecified";

type ToolResultRenderModel = {
  summary: string;
  preview: string[];
  failurePreview?: ToolFailureReason;
  heading?: "command" | "file" | "search" | "diff" | "web" | "subagent" | "json" | "image" | "references";
  streamed?: boolean;
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
  const subagent = items.find((item) => item.type === "subagent_result");
  if (subagent) return buildSubagentModel(subagent.result, status);
  if (lastBashOutput(items)) return buildTaskModel(items, progress, status);
  if (status !== "error") {
    return items.length > 0 ? buildResultContentModel(items, status)
      : buildProgressFallbackModel(progress, fallbackOutput, status);
  }

  const errors: string[] = [];
  const structured = items.filter((item) => {
    if (item.type === "text") {
      errors.push(item.text);
      return false;
    }
    if (item.type === "json") {
      const error = decodeToolError(item.value);
      if (error) {
        errors.push(apiErrorMessage(error));
        return false;
      }
    }
    return true;
  });
  const error = errors.join("\n").trim() || (items.length === 0 ? fallbackOutput?.trim() : undefined);
  const model = structured.length > 0 ? buildResultContentModel(structured, status) : undefined;
  const streamedOutput = buildStreamedOutputBody(progress, true);
  const preview = firstNonEmptyLine(error ?? "");
  return {
    summary: model?.summary ?? toolResultStateLabel(status),
    preview: preview ? [preview] : [],
    body: <>
      {error || (!model?.body && !streamedOutput) ? <ToolResultSection kind="error" failureReason="unspecified">{error}</ToolResultSection> : null}
      {model ? <TypedResultBody model={model} /> : null}
      {streamedOutput}
    </>,
  };
}

function decodeToolError(value: unknown) {
  try {
    return decodeApiError(value);
  } catch {
    // A JSON tool result is not necessarily an ApiError.
    return null;
  }
}

function buildResultContentModel(items: TalosContentItem[], status: ChatToolStatus): ToolResultRenderModel {
  if (items.some((item) => item.type === "diff")) return buildDiffModel(items, status);
  if (items.some((item) => item.type === "file_read")) return buildFileModel(items, status);
  if (items.some((item) => item.type === "search_results")) return buildExploredModel(items, status);
  if (items.some((item) => item.type === "web_reference")) return buildReferenceModel(items, status);
  return buildTextJsonModel(items, status);
}

function TypedResultBody({ model }: { model: ToolResultRenderModel }) {
  const { t } = useTranslation("chat");
  const body = model.body;
  if (!body) return null;
  if (!model.heading) return body;
  const title = t(`toolResultHeading.${model.heading}`);
  return (
    <section aria-label={title} className="space-y-2">
      <h4 className="text-xs font-medium text-muted">{title}</h4>
      {body}
    </section>
  );
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
    heading: "subagent",
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
  live: boolean;
  progress: ToolProgress;
  status: ChatToolStatus;
}) {
  const { t } = useTranslation("chat");
  const model = buildToolResultModel(props);
  const summary = props.compact
    ? model.summary.replace(/^(Queued|Running|Waiting for input|Completed|Failed)(?: · )?/, "")
    : model.summary;
  const body = model.body;
  if (!body) return summary ? <ToolResultSummaryLine summary={summary} /> : null;
  const title = t(model.streamed
    ? props.live && props.status === "running" ? "toolLiveOutput" : "toolStreamedOutput"
    : `toolResultHeading.${model.heading ?? "generic"}`);
  return (
    <section
      aria-label={title}
      className={cn(
        "space-y-3 border-t border-border/65 pt-3",
        props.compact &&
          "[&_[data-tool-result-list=true]]:border-0 [&_[data-tool-result-list=true]]:bg-transparent [&_[data-tool-result-list=true]]:p-0",
      )}
      data-tool-result-renderer={props.compact ? "compact" : "default"}
    >
      <h3 className="text-xs font-semibold text-ink">{title}</h3>
      {summary ? <ToolResultSummaryLine summary={summary} /> : null}
      {body}
    </section>
  );
}

export function ToolResultPreview(props: {
  contentItems?: TalosContentItem[];
  compact?: boolean;
  fallbackOutput?: string;
  progress: ToolProgress;
  status: ChatToolStatus;
}) {
  const { t } = useTranslation("chat");
  const model = buildToolResultModel(props);
  const summary = props.compact && props.status === "error" ? "" : props.compact
    ? model.summary.replace(/^(Queued|Running|Waiting for input|Completed|Failed)(?: · )?/, "")
    : model.summary;
  const preview = model.failurePreview
    ? [[t(`toolFailureReason.${model.failurePreview}`), ...model.preview].join(" ")]
    : model.preview;
  if (preview.length === 0 && !summary) {
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
      {preview.slice(0, props.status === "error" ? 1 : 2).map((line, index) => (
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

function ToolResultSection({ children, kind, failureReason, streamed = false }: {
  children?: string;
  kind: "error" | "stdout" | "stderr" | "log";
  failureReason?: ToolFailureReason;
  streamed?: boolean;
}) {
  const { t } = useTranslation("chat");
  const channel = t(`toolResultSection.${kind}`);
  const title = streamed ? t("toolResultStreamedChannel", { channel }) : channel;
  const text = children?.trim() ? children : failureReason ? t(`toolFailureReason.${failureReason}`) : undefined;
  if (!text) return null;
  return (
    <section aria-label={title} className="space-y-1" data-tool-result-section={kind}>
      <h4 className={cn("text-xs font-medium", kind === "error" ? "text-danger" : "text-muted")}>{title}</h4>
      <pre className={cn(
        "max-h-80 overflow-auto rounded-lg border p-3 font-mono text-[12px] leading-5",
        kind === "error" ? "whitespace-pre-wrap break-words border-danger/30 bg-danger/5 text-danger" : "border-border/70 bg-surface/85 text-ink",
      )}>{text}</pre>
    </section>
  );
}

export function lastBashOutput(items: TalosContentItem[] | undefined) {
  return items?.filter((item) => item.type === "bash_output").at(-1);
}

function buildTaskModel(
  items: TalosContentItem[],
  progress: ToolProgress,
  status: ChatToolStatus,
): ToolResultRenderModel {
  const taskItems = items.filter((item) => item.type === "bash_output");
  const capturedStdout = taskItems.map((item) => item.stdout).filter(Boolean).join("\n");
  const capturedStderr = taskItems.map((item) => item.stderr).filter(Boolean).join("\n");
  const taskStatus = taskItems.at(-1)?.status;
  const truncated = taskItems.some((item) => item.truncated);
  const logs = taskItems.flatMap((item) => item.outputLog ? [item.outputLog] : []);
  const mayLackOutput = !capturedStdout && !capturedStderr ||
    status === "error" && taskStatus === "completed" || truncated ||
    logs.some((log) => log.captureStatus !== "complete");
  const stdout = capturedStdout || (mayLackOutput ? collectToolOutput(progress, "stdout") : "");
  const stderr = capturedStderr || (mayLackOutput ? collectToolOutput(progress, "stderr") : "");
  const failureReason: ToolFailureReason | undefined = status !== "error" ? undefined
    : taskStatus === "timed_out" ? "timedOut"
    : taskStatus === "cancelled" ? "cancelled"
    : taskStatus === "failed" ? "failed"
    : taskStatus === "completed" ? "captureFailed" : "unspecified";
  const stderrTail = lastNonEmptyLine(stderr);
  const preview = failureReason === "failed" ? stderrTail ?? lastNonEmptyLine(stdout)
    : failureReason ? undefined : firstNonEmptyLine(stdout || stderr);
  return {
    heading: "command",
    summary: "",
    preview: preview ? [preview] : [],
    failurePreview: failureReason === "failed" && stderrTail ? undefined : failureReason,
    body: failureReason || truncated || stdout.trim() || stderr.trim() || logs.length > 0 ? <>
      {failureReason ? <ToolResultSection kind="error" failureReason={failureReason} /> : null}
      {truncated ? <TruncatedOutputNotice /> : null}
      <ToolResultSection kind="stdout" streamed={!capturedStdout}>{stdout}</ToolResultSection>
      <ToolResultSection kind="stderr" streamed={!capturedStderr}>{stderr}</ToolResultSection>
      {logs.length > 0 ? <SavedOutputLog logs={logs} /> : null}
    </> : null,
  };
}

function TruncatedOutputNotice() {
  const { t } = useTranslation("chat");
  return <p className="text-xs text-muted">{t("toolResultTruncated")}</p>;
}

function SavedOutputLog({ logs }: {
  logs: NonNullable<Extract<TalosContentItem, { type: "bash_output" }>["outputLog"]>[];
}) {
  const { t, i18n } = useTranslation("chat");
  const lines = logs.map((log) => `${log.path}\n${t("toolSavedOutputLog.summary", {
    storedBytes: log.storedBytes,
    totalBytes: log.totalBytes,
    status: t(`toolSavedOutputLog.captureStatus.${log.captureStatus}`),
    expires: new Date(log.expiresAt * 1000).toLocaleString(i18n.resolvedLanguage),
  })}`);
  return <ToolResultSection kind="log">{lines.join("\n\n")}</ToolResultSection>;
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
    heading: "diff",
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
    lines.push(item.limit === 0 ? `${item.path} · byte range` : `${item.path}:${start}-${Math.min(item.offset + item.limit, item.total_lines ?? Number.MAX_SAFE_INTEGER)}`);
    if (item.nextByteOffset !== null && item.nextByteOffset !== undefined) {
      lines.push(`Continue with byte_offset=${item.nextByteOffset}`);
    }
    for (const [index, line] of item.content.split("\n").entries()) {
      count += 1;
      lines.push(`${String(start + index).padStart(4, " ")} ${line}`);
    }
  }
  const summary = `${toolResultStateLabel(status)} · ${count} lines`;
  return {
    heading: "file",
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
      if (item.matches.length === 0 && item.total_matches !== null) {
        count += item.total_matches;
        lines.push(`${item.total_matches} match(es)${item.partial ? " (partial)" : ""}`);
      }
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
    heading: "search",
    summary,
    preview: lines.filter(Boolean).slice(0, 2),
    body: <ResultList lines={lines} />,
  };
}

function buildReferenceModel(items: TalosContentItem[], status: ChatToolStatus): ToolResultRenderModel {
  const preview = items.flatMap((item) => item.type === "web_reference" && item.title && item.title !== item.url
    ? [item.title] : []);
  return {
    heading: "web",
    summary: toolResultStateLabel(status),
    preview: preview.slice(0, 2),
    get body() {
      const lines = items.flatMap((item) => {
        if (item.type === "web_reference") {
          return [item.title, item.source_domain, item.url, item.snippet].filter((line): line is string => Boolean(line));
        }
        return [formatReference(item)].filter((line): line is string => line !== undefined);
      });
      const content = items.filter((item) =>
        item.type === "text" || item.type === "json" || item.type === "image" || item.type === "progress");
      return (
        <div className="space-y-2">
          <div className="[overflow-wrap:anywhere]"><ResultList lines={Array.from(new Set(lines))} /></div>
          {content.length > 0 ? buildTextJsonModel(content, status).body : null}
        </div>
      );
    },
  };
}

function formatReference(item: TalosContentItem) {
  if (item.type === "artifact_ref") {
    return `artifact: ${item.label ?? item.artifact_id}${item.mime_type ? ` · ${item.mime_type}` : ""}`;
  }
  if (item.type === "mcp_resource_ref") {
    return `mcp: ${item.server}:${item.uri}${item.mime_type ? ` · ${item.mime_type}` : ""}`;
  }
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
    const reference = formatReference(item);
    return reference ? [reference] : [];
  });
  const text = lines.join("\n");
  const images = items.filter((item) => item.type === "image");
  const summary = toolResultStateLabel(status);
  const preview = firstNonEmptyLine(text);
  const heading = items.every((item) => item.type === "json") ? "json"
    : items.every((item) => item.type === "image") ? "image"
    : items.every((item) => item.type === "artifact_ref" || item.type === "mcp_resource_ref") ? "references"
    : undefined;
  return {
    heading,
    summary,
    preview: preview && preview !== "{" && preview !== "[" ? [preview] : [],
    body: text.trim() || images.length > 0 ? <div className="flex flex-col gap-3">
      {text.trim() && <pre className="max-h-80 overflow-auto rounded-lg border border-border/70 bg-surface/85 p-3 font-mono text-[12px] leading-5 text-ink">{text}</pre>}
      {images.map((item) => (
        <FileImage key={item.fileId} fileId={item.fileId} label={item.label} />
      ))}
    </div> : null,
  };
}

function collectToolOutput(progress: ToolProgress, kind?: "stdout" | "stderr") {
  return progress.filter((entry) => kind ? entry.kind === kind : entry.kind !== "status")
    .map((entry) => entry.message)
    .join("");
}

function buildStreamedOutputBody(progress: ToolProgress, streamedPreview = false) {
  const stdout = collectToolOutput(progress, "stdout");
  const stderr = collectToolOutput(progress, "stderr");
  if (!stdout.trim() && !stderr.trim()) return null;
  return (
    <>
      <ToolResultSection kind="stdout" streamed={streamedPreview}>{stdout}</ToolResultSection>
      <ToolResultSection kind="stderr" streamed={streamedPreview}>{stderr}</ToolResultSection>
    </>
  );
}

function buildProgressFallbackModel(
  progress: ToolProgress,
  fallbackOutput: string | undefined,
  status: ChatToolStatus,
): ToolResultRenderModel {
  const resultText = fallbackOutput?.trim() ? fallbackOutput : undefined;
  const output = resultText ?? collectToolOutput(progress);
  const streamedBody = resultText ? null : buildStreamedOutputBody(progress);
  const summary = toolResultStateLabel(status);
  const preview = status === "running"
    ? output.trim().split(/\r?\n/).at(-1)
    : firstNonEmptyLine(output);
  return {
    summary,
    preview: preview ? [preview] : [],
    streamed: streamedBody !== null,
    body: resultText ? (
      <pre className="max-h-80 overflow-auto rounded-lg border border-border/70 bg-surface/85 p-3 font-mono text-[12px] leading-5 text-ink">
        {resultText}
      </pre>
    ) : streamedBody,
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
    case "cancelled":
      return "Cancelled";
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

function lastNonEmptyLine(text: string) {
  return text.trimEnd().split("\n").at(-1)?.trim() || undefined;
}
