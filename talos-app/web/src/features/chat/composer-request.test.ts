import { expect, it } from "vitest";
import { createInitialTurnRequest } from "./api";
import { restoreComposerRequest } from "./composer-request";

it("restores the exact request needed to retry composer admission", () => {
  const request = createInitialTurnRequest("session-1", "Continue", {
    nodeId: "computer-1", fileIds: ["file-1"], skillSelection: { name: "goal", path: "/skills/goal/SKILL.md" },
  });
  expect(restoreComposerRequest(JSON.stringify(request))).toEqual(request);
});

it.each([
  null,
  { input: { content: "invalid" } },
  { input: { content: [{ type: "file", fileId: 7 }] } },
  { modelSelection: { modelRef: { provider: "test" }, reasoningEffort: "high" } },
])("rejects corrupt saved requests before rendering or replay", (invalid) => {
  const request = createInitialTurnRequest("session-1", "Continue");
  const body = invalid === null ? null : { ...request, ...invalid };
  expect(() => restoreComposerRequest(JSON.stringify(body))).toThrow("Invalid saved composer");
});


it.each(["askForApproval", "approveForMe", "fullAccess"] as const)("restores the %s permission mode", (mode) => {
  const request = createInitialTurnRequest("session-1", "Continue");
  request.userPermissionMode = mode;
  expect(restoreComposerRequest(JSON.stringify(request))).toEqual(request);
});

it.each(["light", "medium", "high", "xhigh"] as const)("restores the %s reasoning effort", (effort) => {
  const request = createInitialTurnRequest("session-1", "Continue");
  request.modelSelection = { modelRef: { provider: "fixture", id: "model" }, reasoningEffort: effort };
  expect(restoreComposerRequest(JSON.stringify(request))).toEqual(request);
});

it("rejects inherited object property names as permission modes", () => {
  const request = createInitialTurnRequest("session-1", "Continue");
  expect(() => restoreComposerRequest(JSON.stringify({ ...request, userPermissionMode: "toString" })))
    .toThrow("Invalid saved composer request.");
});
