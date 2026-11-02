import type { AppProtocol as API } from "@talos/sdk-core";
export type TraceSelection =
  | {
      kind: "request";
      turnId: string;
      modelRequestId: string;
      callRecordId?: string;
      view?: "prompt" | "response";
      messageIndex?: number;
    }
  | {
      kind: "tool";
      turnId: string;
      callId: string;
    }
  | {
      kind: "fact";
      turnId: string;
      block: API.AgentMessageBlock;
    };
export type TraceTurn = {
  ordinal: number;
  identity: API.AgentTurnRef;
  messages: API.AgentMessage[];
  metrics?: API.AgentRunMetrics;
  result?: API.AgentRunResult;
  status: string;
};
export function traceTurns(history: API.SessionHistoryResponse | undefined): TraceTurn[] {
  return (history?.data ?? []).map((turn, index) => ({
    ordinal: index + 1,
    identity: turn.identity,
    messages: turn.result.result.messages,
    metrics: turn.result.result.metrics,
    result: turn.result,
    status: turn.result.status,
  }));
}
export function blockLabel(block: API.AgentMessageBlock): string {
  const c = block.content;
  return c.type === "assistantText"
    ? c.data.phase
    : c.type === "userContent"
      ? "user"
      : c.type;
}
export function snapshotMessageKind(message: API.SnapshotMessage): string {
  if (message.role !== "user") return message.role;
  return message.inputOrigin?.kind === "context"
    ? "context"
    : message.inputOrigin?.kind === "user"
      ? "userContent"
      : "input";
}
export function blockText(block: API.AgentMessageBlock): string {
  const c = block.content;
  switch (c.type) {
    case "assistantText":
    case "reasoning":
    case "executionSummary":
      return c.data.text;
    case "userContent":
      return c.data.type === "text"
        ? c.data.text
        : JSON.stringify(c.data, null, 2);
    case "toolCall":
      return `${c.data.name}\n${toolArgumentsText(c.data.arguments)}\n${c.data.progress.map((p) => `[${p.kind} · ${p.occurredAt}] ${p.text}`).join("\n")}`;
    case "toolResult":
      return `${c.data.toolName}\n${c.data.content.map((c) => (c.type === "text" ? c.text : JSON.stringify(c, null, 2))).join("\n")}`;
    default:
      return JSON.stringify(c.data, null, 2);
  }
}
export function toolArgumentsText(
  argumentsValue: API.AgentToolCallArguments,
): string {
  return argumentsValue.state === "complete"
    ? JSON.stringify(argumentsValue.value, null, 2)
    : argumentsValue.jsonFragment;
}
export function canonicalTool(turn: TraceTurn | undefined, callId: string) {
  const blocks = turn?.messages.flatMap((m) => m.blocks) ?? [];
  const call = blocks.find(
    (b) => b.content.type === "toolCall" && b.content.data.callId === callId,
  )?.content;
  const result = blocks.find(
    (b) => b.content.type === "toolResult" && b.content.data.callId === callId,
  )?.content;
  return {
    call: call?.type === "toolCall" ? call.data : null,
    result: result?.type === "toolResult" ? result.data : null,
  };
}
/** Each observation contributes once; never combine these with canonical Turn totals. */
export function observedTotals(entries: API.TrajectoryEntry[], turn?: TraceTurn) {
  const requests = entries.flatMap((e) =>
    e.type === "modelRequest" ? [e.request] : [],
  );
  const callRecords = requests.flatMap((r) => r.callRecords);
  const tools = entries.flatMap((e) => (e.type === "tool" ? [e.tool] : []));
  const sum = (values: Array<number | null | undefined>) =>
    values.some((v) => v != null)
      ? values.reduce<number>((n, v) => n + (v ?? 0), 0)
      : null;
  const execution = tools.map((tool) => canonicalTool(turn, tool.callId).result?.metrics);
  return {
    requests: requests.length,
    llmMs: sum(callRecords.map((a) => a.metrics?.executionDurationMs)),
    toolMs: sum(execution.map((i) => i?.executionDurationMs)),
    tokens: sum(callRecords.map((a) => a.metrics?.usage?.total_tokens)),
    partial:
      callRecords.some(
        (a) => a.metrics?.usage == null || a.metrics?.executionDurationMs == null,
      ) ||
      execution.some((i) => i?.executionDurationMs == null),
  };
}
