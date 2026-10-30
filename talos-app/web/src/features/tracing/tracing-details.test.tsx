// @vitest-environment jsdom
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { TalosApiError, type AppProtocol as API } from "@talos/sdk-core";
import { installTracingDom, cleanupTracingDom } from "./tracing.test-utils";
import { i18n } from "@/shared/i18n/setup";
import { SessionTracingPanel } from "./components/SessionTracingPanel";
import { RequestSnapshotView } from "./components/RequestSnapshotView";
import { history, identity, request, prompt } from "./tracing.fixtures";

const mocks = vi.hoisted(() => ({
  capabilities: vi.fn(), readTurn: vi.fn(), readRequest: vi.fn(),
  readTool: vi.fn(), payload: vi.fn(),
}));
vi.mock("@/shared/api/client", () => ({ getTalosClient: () => ({ trajectory: mocks }) }));
vi.mock("@/features/chat", () => ({ useConnectionKey: () => "details-connection" }));
const toolMessage: API.SnapshotMessage = {
  index: 8, role: "tool", name: "Read", toolCallId: "call-read",
  content: "consumed tool body", reasoning: null, toolCalls: null,
  attachments: null, cacheControl: { type: "none" },
  inputOrigin: { kind: "context", source: "runtime", admission: "unknown" },
};
const rawArguments = '{ "path" : "checkout.py" }\n';
const toolBlocks: API.AgentMessageBlock[] = [
  {
    blockId: "tool-call", status: "completed", startedAt: 1, completedAt: 2,
    content: { type: "toolCall", data: {
      callId: "call-read", name: "Read", arguments: { state: "complete", value: { path: "checkout.py" } }, progress: [],
    } },
  },
  {
    blockId: "tool-result", status: "failed", startedAt: 1, completedAt: 2,
    content: { type: "toolResult", data: {
      callId: "call-read", toolName: "Read", isError: true,
      content: [{ type: "text", text: "permission refused" }],
    } },
  },
];
beforeEach(async () => {
  vi.clearAllMocks();
  await i18n.changeLanguage("zh-CN");
  installTracingDom({ narrow: true });
  mocks.capabilities.mockResolvedValue({ supported: true, });
  const tool: API.TrajectoryTool = {
    callId: "call-read", sourceModelRequestId: "r", sourceCallRecordId: "a", intervals: null,
  };
  mocks.readTurn.mockResolvedValue({
    identity, clockSegments: [],
    data: [{ type: "modelRequest", request }, { type: "tool", tool }], nextCursor: null,
  });
  const ref = (payloadId: string) => ({ payloadId, complete: true });
  mocks.readTool.mockResolvedValue({
    identity, tool, preparedResult: ref("prepared"),
    usages: [{ modelRequestId: "consumer", messageIndex: 8, usage: "direct" }],
  });
  mocks.readRequest.mockImplementation(async ({ modelRequestId }) => ({
    identity, request: { ...request, modelRequestId }, prompt: ref("prompt"),
    tools: null,
    parameters: null,
    outputs: [{ callRecordId: "a", response: ref("response") }],
  }));
  mocks.payload.mockImplementation(async (_identity, ref) => {
    if (ref.payloadId === "response") return {
      kind: "response", parts: [{ type: "toolCall", call: {
        callId: "call-read", name: "Read", arguments: { path: "checkout.py" },
        argumentsText: rawArguments,
      } }],
    };
    if (ref.payloadId === "prepared") return {
      kind: "preparedResult", message: {
        ...toolMessage, index: 0, content: "prepared tool body",
        attachments: [{ fileId: "saved-image", filename: null, kind: "image", mimeType: null, sizeBytes: null, availability: "available" }],
      },
    };
    return { kind: "prompt", messages: [toolMessage] };
  });
});
afterEach(() => {
  cleanupTracingDom();
});

it("reads exact tool arguments, canonical error status and the chosen consuming message", async () => {
  const loadedHistory: API.SessionHistoryResponse = {
    ...history,
    data: history.data.map((turn) => ({
      ...turn,
      result: { status: "completed", result: {
        ...turn.result.result,
        completion: { stopReason: "outputLimit" },
        metrics: { ...turn.result.result.metrics, context: {
          usedTokens: 13980, contextWindowTokens: 1000000,
          effectivePromptBudgetTokens: 987712, measuredAt: 1793270546,
        } },
        messages: [{ ...turn.result.result.messages[0], blocks: toolBlocks }],
      } },
    })),
  };
  render(<QueryClientProvider client={new QueryClient()}>
    <SessionTracingPanel sessionId="s" visible history={loadedHistory} />
  </QueryClientProvider>);
  expect(screen.getByText("轮次结果: 达到输出上限")).toBeTruthy();
  fireEvent.click(screen.getByText("实测上下文窗口"));
  expect(screen.getByText("13,980")).toBeTruthy();
  mocks.readRequest.mockRejectedValueOnce(new Error("temporary tool request failure"));
  fireEvent.click((await screen.findAllByRole("button", { name: /TOOL.*Read/ }))[0]);
  fireEvent.click(await screen.findByRole("button", { name: "重试读取" }));
  const panel = screen.getByRole("tabpanel");
  expect(await within(panel).findByText("r")).toBeTruthy();
  expect(within(panel).getByText("a")).toBeTruthy();
  await waitFor(() => expect(within(panel).getByText(rawArguments.trim()).textContent).toBe(rawArguments));
  fireEvent.click(screen.getByRole("tab", { name: "实际执行" }));
  expect(within(panel).getByText("状态: 错误结果")).toBeTruthy();
  expect(within(panel).getByText("permission refused")).toBeTruthy();
  fireEvent.click(screen.getByRole("tab", { name: "模型结果" }));
  expect(await within(panel).findByText("prepared tool body")).toBeTruthy();
  expect(within(panel).getByText("Read")).toBeTruthy();
  expect(within(panel).getByText("saved-image")).toBeTruthy();
  expect(within(panel).getByText("MIME 类型").nextElementSibling?.textContent).toBe("—");
  fireEvent.click(screen.getByRole("combobox", { name: "消费请求" }));
  fireEvent.click(await screen.findByRole("option", { name: "consumer · 直接使用" }));
  expect(await within(panel).findByText("consumed tool body")).toBeTruthy();
  expect(within(panel).getByText("#8")).toBeTruthy();
  expect(within(panel).getByText("系统运行信息")).toBeTruthy();
  expect(mocks.readRequest).toHaveBeenCalledWith({ identity, modelRequestId: "consumer" }, expect.any(AbortSignal));
});

it("preserves unknown Prompt origins and the identity of recorded response tool calls", async () => {
  const { rerender } = render(<RequestSnapshotView value={prompt} reference={{ payloadId: "snapshot", complete: true }} />);
  expect(screen.getByText("SYSTEM")).toBeTruthy();
  expect(screen.getByText("部分消息未保存上下文来源，正文仍按原文展示。")).toBeTruthy();
  const userMessage: API.SnapshotMessage = {
    ...toolMessage, index: 0, role: "user", name: null, toolCallId: null,
    content: "unknown input", inputOrigin: null,
  };
  rerender(<RequestSnapshotView value={{ kind: "prompt", messages: [
    userMessage,
    { ...userMessage, index: 1, content: "runtime context", inputOrigin: { kind: "context", source: "runtime", admission: "introduced" } },
    { ...userMessage, index: 2, content: "human input", inputOrigin: { kind: "user", source: "user", admission: "introduced" } },
    { ...userMessage, index: 3, content: "unknown origin", inputOrigin: { kind: "context", source: "unknown", admission: "unknown" } },
  ] }} reference={{ payloadId: "snapshot", complete: true }} />);
  for (const [content, badge] of [["unknown input", "INPUT"], ["runtime context", "CONTEXT"], ["human input", "USER"]]) {
    const message = within(screen.getByText(content).closest("section")!);
    expect(message.getByText(badge)).toBeTruthy();
    expect(message.getByTitle("消息角色: user")).toBeTruthy();
  }
  expect(screen.getAllByText("部分消息未保存上下文来源，正文仍按原文展示。")).toHaveLength(1);
  expect(screen.queryByText("未知来源")).toBeNull();
  rerender(<RequestSnapshotView value={{ kind: "prompt", messages: [{
    ...userMessage, role: "assistant", content: "tool request", reasoning: [
      { type: "text", text: "exact public replay" },
      { type: "summary", text: "exact replay summary" },
    ],
  }] }} reference={{ payloadId: "with-reasoning", complete: true }} />);
  fireEvent.click(screen.getByText("回传的公开思考"));
  expect(screen.getByText("exact public replay")).toBeTruthy();
  expect(screen.getByText("exact replay summary")).toBeTruthy();
  rerender(<RequestSnapshotView value={{ kind: "response", parts: [{ type: "toolCall", call: {
    callId: "response-call", name: "Read", arguments: { path: "checkout.py" }, argumentsText: rawArguments,
  } }] }} reference={{ payloadId: "snapshot", complete: true }} />);
  expect(screen.getByText("response-call")).toBeTruthy();
  expect(screen.getByText(rawArguments.trim()).textContent).toBe(rawArguments);
  const interrupted = ' { "path" : ';
  rerender(<RequestSnapshotView value={{ kind: "response", parts: [{ type: "toolCall", call: {
    callId: "", name: "", arguments: null, argumentsText: interrupted,
  } }] }} reference={{ payloadId: "snapshot", complete: false }} />);
  expect(screen.getByRole("heading", { name: "—" })).toBeTruthy();
  expect(screen.getByText("—", { selector: "small" })).toBeTruthy();
  expect(screen.getByText(interrupted.trim()).textContent).toBe(interrupted);
  expect(screen.getByText("已保存输出不完整")).toBeTruthy();
  rerender(<RequestSnapshotView value={prompt} reference={{ payloadId: "old-snapshot", complete: null }} />);
  expect(screen.getByText("内容完整性: —")).toBeTruthy();
  rerender(<RequestSnapshotView value={null} reference={null} />);
  expect(screen.getByText("—")).toBeTruthy();
});

it("keeps canonical failed Turn outcomes and a missing observation result blank", async () => {
  mocks.readTurn.mockResolvedValue({ identity, clockSegments: [], data: null, nextCursor: null });
  const failed: API.SessionHistoryResponse = {
    ...history, data: [{ ...history.data[0], result: { status: "failed", result: {
      ...history.data[0].result.result,
      failure: { retryable: false, error: { code: "permissionDenied", message: "Saved turn failure", localization: null } },
    } } }],
  };
  render(<QueryClientProvider client={new QueryClient()}>
    <SessionTracingPanel sessionId="s" visible history={failed} />
  </QueryClientProvider>);
  expect(screen.getByText("轮次结果: Saved turn failure")).toBeTruthy();
  expect(await screen.findByText("请求明细未采集；已有会话事实仍可查看。")).toBeTruthy();
  expect(screen.getByText("阶段反馈保持原文")).toBeTruthy();
  expect(mocks.readRequest).not.toHaveBeenCalled();
});

it("shows linked tools once without hiding canonical tools from legacy or missing supplements", async () => {
  const savedHistory: API.SessionHistoryResponse = {
    ...history, data: history.data.map((turn) => ({
      ...turn, result: { status: "completed", result: {
        ...turn.result.result, completion: { stopReason: "endTurn" },
        messages: [{ ...turn.result.result.messages[0], blocks: toolBlocks }],
      } },
    })),
  };
  const tool: API.TrajectoryEntry = { type: "tool", tool: {
    callId: "call-read", sourceModelRequestId: "r", sourceCallRecordId: "a", intervals: null,
  } };
  for (const scenario of ["linked", "unknownSource", "partialMissing", "readFailure", "legacy"]) {
    mocks.readTool.mockClear();
    if (scenario === "readFailure")
      mocks.readTool.mockRejectedValueOnce(new Error("temporary supplement failure"));
    if (scenario === "partialMissing")
      mocks.readTool.mockRejectedValueOnce(new TalosApiError({ code: "resourceNotFound", message: "Tool supplement absent", localization: null }));
    mocks.readTurn.mockResolvedValue({
      identity, clockSegments: [], nextCursor: null,
      data: scenario === "legacy" ? null : scenario === "unknownSource" ? [{ ...tool, tool: { ...tool.tool, sourceModelRequestId: null, sourceCallRecordId: null } }]
        : scenario === "linked" ? [{ type: "modelRequest", request }, tool] : [{ type: "modelRequest", request }],
    });
    const { unmount } = render(<QueryClientProvider client={new QueryClient()}>
      <SessionTracingPanel sessionId="s" visible history={savedHistory} />
    </QueryClientProvider>);
    if (scenario === "legacy") await screen.findByText("请求明细未采集；已有会话事实仍可查看。");
    else if (scenario === "unknownSource") await screen.findByRole("button", { name: /错误结果/ });
    else await screen.findByText("模型请求 1");
    const ledger = screen.getByLabelText("连续执行记录");
    expect(within(ledger).getAllByRole("button", { name: /TOOL.*Read/ })).toHaveLength(
      ["linked", "unknownSource"].includes(scenario) ? 1 : 2,
    );
    if (["legacy", "partialMissing", "readFailure"].includes(scenario)) {
      fireEvent.click(within(ledger).getAllByRole("button", { name: /TOOL.*Read/ })[0]);
      if (scenario === "readFailure") {
        expect(await screen.findByRole("alert")).toBeTruthy();
      } else if (scenario === "partialMissing") {
        await waitFor(() => expect(mocks.readTool).toHaveBeenCalledTimes(1));
        await waitFor(() => expect(screen.queryByText("加载中…")).toBeNull());
      } else expect(mocks.readTool).not.toHaveBeenCalled();
      fireEvent.click(screen.getByRole("tab", { name: "实际执行" }));
      expect(within(screen.getByRole("tabpanel")).getByText("permission refused")).toBeTruthy();
      expect(screen.queryByRole("alert")).toBeNull();
    }
    unmount();
  }
});
