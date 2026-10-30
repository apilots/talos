import type { AppProtocol as API } from "@talos/sdk-core";
import type { TraceTurn } from "./selectors";
import { blockLabel, blockText } from "./selectors";

/** Copies resident facts only, including related details read outside the ledger page. */
export function composeLoadedTurn(
  turn: TraceTurn,
  requests: API.TrajectoryReadModelRequestResponse[],
  tools: API.TrajectoryReadToolResponse[],
  payloads: Map<string, API.TrajectoryPayload>,
  label: (key: string) => string,
  entries: API.TrajectoryEntry[],
): string {
  const parts = [
    `Turn ${turn.identity.turnId}\n${label("copyScope")}`,
    `${label("originalScope")}\n${JSON.stringify(turn.metrics ?? null, null, 2)}`,
    ...(turn.result
      ? [
          `${label("turnResult")}\n${JSON.stringify(
            turn.result.status === "completed"
              ? turn.result.result.completion
              : turn.result.result.failure,
            null,
            2,
          )}`,
        ]
      : []),
  ];
  for (const message of turn.messages)
    for (const block of message.blocks)
      parts.push(
        `[${label(blockLabel(block))} · ${block.status}]\n${blockText(block)}`,
      );
  const requestDetails = new Map(requests.map((r) => [r.request.modelRequestId, r]));
  const toolDetails = new Map(tools.map((t) => [t.tool.callId, t]));
  const seenRequests = new Set<string>(), seenTools = new Set<string>();
  const snapshot = (ref: API.PayloadRef | null) =>
    ref == null
      ? "—"
      : `${JSON.stringify(ref, null, 2)}\n${payloads.has(ref.payloadId) ? JSON.stringify(payloads.get(ref.payloadId), null, 2) : label("notLoaded")}`;
  const request = (entry: API.ModelRequestTrace) => {
    seenRequests.add(entry.modelRequestId);
    const detail = requestDetails.get(entry.modelRequestId);
    parts.push(
      `[${label("llm")} ${entry.ordinal}]\n${JSON.stringify(detail?.request ?? entry, null, 2)}`,
    );
    if (!detail) {
      parts.push(label("notLoaded"));
      return;
    }
    const refs: Array<[string, API.PayloadRef | null]> = [
      ["prompt", detail.prompt],
      ["tools", detail.tools],
      ["parameters", detail.parameters],
      ...detail.outputs.map(
        (o): [string, API.PayloadRef | null] => [
          `${label("response")} · ${o.callRecordId}`,
          o.response,
        ],
      ),
    ];
    for (const [name, ref] of refs) parts.push(`[${name}]\n${snapshot(ref)}`);
  };
  const tool = (entry: API.TrajectoryTool) => {
    seenTools.add(entry.callId);
    const detail = toolDetails.get(entry.callId);
    parts.push(
      `[${label("tool")} ${entry.callId}]\n${JSON.stringify(detail ?? entry, null, 2)}\n${snapshot(detail?.preparedResult ?? null)}`,
    );
  };
  for (const entry of entries) {
    if (entry.type !== "modelRequest") continue;
    request(entry.request);
    for (const callRecord of entry.request.callRecords)
      for (const candidate of entries)
        if (candidate.type === "tool" &&
          candidate.tool.sourceModelRequestId === entry.request.modelRequestId &&
          candidate.tool.sourceCallRecordId === callRecord.callRecordId)
          tool(candidate.tool);
  }
  for (const entry of entries)
    if (entry.type === "tool" && !seenTools.has(entry.tool.callId)) tool(entry.tool);
  const relatedRequests = requests.filter(
    (r) => !seenRequests.has(r.request.modelRequestId),
  );
  const relatedTools = tools.filter((t) => !seenTools.has(t.tool.callId));
  if (relatedRequests.length || relatedTools.length) {
    parts.push(label("loadedRelatedDetails"));
    for (const detail of relatedRequests) request(detail.request);
    for (const detail of relatedTools) tool(detail.tool);
  }
  return parts.join("\n\n") + "\n";
}
