import { expect, it } from "vitest";
import type { AppProtocol as API } from "@talos/sdk-core";
import { history, identity, request, ref } from "../tracing.fixtures";
import { composeLoadedTurn } from "./copy";
import { observedTotals, traceTurns } from "./selectors";

it("copies ledger order regardless of detail read order and marks unloaded snapshots", () => {
  const second = { ...request, modelRequestId: "second", ordinal: 2 };
  const unread = { ...request, modelRequestId: "unread", ordinal: 3 };
  const related = { ...request, modelRequestId: "off-page-consumer", ordinal: 4 };
  const detail = (
    value: API.ModelRequestTrace,
  ): API.TrajectoryReadModelRequestResponse => ({
    identity,
    request: value,
    prompt: ref,
    tools: null,
    parameters: null,
    outputs: [],
  });
  const copied = composeLoadedTurn(
    traceTurns(history)[0],
    [detail(second), detail(request), { ...detail(related), prompt: { payloadId: "off-page-input", complete: false } }],
    [],
    new Map([["off-page-input", { kind: "prompt", messages: [{ index: 0, role: "tool", content: "exact off-page consumed body", reasoning: null, name: "Read", toolCallId: "call-between-requests", toolCalls: null, attachments: null, cacheControl: null, inputOrigin: null }] }]]),
    (key) => key,
    [
      { type: "modelRequest", request },
      { type: "modelRequest", request: second },
      { type: "modelRequest", request: unread },
      {
        type: "tool",
        tool: {
          callId: "call-between-requests",
          sourceModelRequestId: request.modelRequestId,
          sourceCallRecordId: "a",
          intervals: null,
        },
      },
    ],
  );
  expect(copied).toContain('"modelRequestId": "r"');
  expect(copied.indexOf("[llm 1]")).toBeLessThan(
    copied.indexOf("[tool call-between-requests]"),
  );
  expect(copied.indexOf("[tool call-between-requests]")).toBeLessThan(
    copied.indexOf("[llm 2]"),
  );
  expect(copied.indexOf("[llm 2]")).toBeLessThan(copied.indexOf("[llm 3]"));
  expect(copied).toContain("阶段反馈保持原文");
  expect(copied).toContain("notLoaded");
  expect(copied).toContain("originalScope");
  expect(copied).toContain("loadedRelatedDetails");
  expect(copied).toContain("exact off-page consumed body");
  expect(copied).toContain('"complete": false');
  expect(copied.indexOf("loadedRelatedDetails")).toBeGreaterThan(copied.indexOf("[llm 3]"));
});
it("keeps observed totals separate and marks missing callRecords partial", () => {
  const totals = observedTotals([
    { type: "modelRequest", request },
    {
      type: "modelRequest",
      request: {
        ...request,
        modelRequestId: "second",
        callRecords: [
          { ...request.callRecords[0], callRecordId: "b", usage: null, timing: null },
        ],
      },
    },
  ]);
  expect(totals.tokens).toBe(420);
  expect(totals.llmMs).toBe(1000);
  expect(totals.requests).toBe(2);
  expect(totals.partial).toBe(true);
});
