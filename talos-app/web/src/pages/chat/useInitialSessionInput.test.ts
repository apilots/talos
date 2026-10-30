// @vitest-environment jsdom

import { renderHook, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { ModelInfo } from "@talos/sdk-browser";

import {
  hasInitialSessionInput,
  useInitialSessionInput,
} from "./useInitialSessionInput";

describe("useInitialSessionInput", () => {
  it("sends an attachment-only navigation draft once", async () => {
    const locationState = {
      initialPrompt: "",
      initialPermissionMode: "approveForMe" as const,
      initialAttachments: [
        {
          fileId: "file-1",
          scope: { type: "personal" as const },
          originalFilename: "diagram.png",
          mimeType: "image/png",
          sizeBytes: 8,
          tags: [],
          metadata: {},
          createdAt: 1,
          updatedAt: 1,
        },
      ],
    };
    const sendText = vi.fn(() => new Promise<void>(() => undefined));
    const navigate = vi.fn();

    renderHook(() =>
      useInitialSessionInput({
        onPendingSessionChange: vi.fn(),
        locationPathname: "/sessions/sess-1",
        locationState,
        navigate,
        projects: [],
        sendText,
        sessionId: "sess-1",
        setDraft: vi.fn(),
      }),
    );

    await waitFor(() => {
      expect(sendText).toHaveBeenCalledWith("", {
        modelSelection: undefined,
        userPermissionMode: "approveForMe",
        project: undefined,
        fileIds: ["file-1"],
      });
    });
    expect(hasInitialSessionInput(locationState)).toBe(true);
    expect(navigate).toHaveBeenCalledWith("/sessions/sess-1", { replace: true, state: null });
  });

  it("waits for catalog capabilities and carries the qualified model plus Off from navigation into the first turn", async () => {
    const models = [{ provider: "llm-gateway", id: "model", name: "Local", context_window: 131072, speed: "standard", input: ["text"], cost: { input: 0, output: 0, cache_read: 0, cache_write: 0 }, request_parameters: {}, reasoning: true, reasoning_effort: "high", selection_capabilities: { reasoning: "toggle", reasoning_efforts: [], fast: true, default_fast: false, default_reasoning_effort: null } }] as ModelInfo[];
    const sendText = vi.fn(() => new Promise<void>(() => undefined));
    const navigate = vi.fn();
    const { rerender } = renderHook(({ catalog }: { catalog?: ModelInfo[] }) => useInitialSessionInput({
      onPendingSessionChange: vi.fn(), locationPathname: "/sessions/sess-1", locationState: { initialPrompt: "hello", initialModel: { provider: "llm-gateway", id: "model" }, initialReasoningLevel: "none" },
      models: catalog, navigate, projects: [], sendText, sessionId: "sess-1", setDraft: vi.fn(),
    }), { initialProps: { catalog: undefined as ModelInfo[] | undefined } });
    expect(sendText).not.toHaveBeenCalled();
    expect(navigate).not.toHaveBeenCalled();
    rerender({ catalog: models });
    await waitFor(() => expect(sendText).toHaveBeenCalledExactlyOnceWith("hello", expect.objectContaining({ modelSelection: { modelRef: { provider: "llm-gateway", id: "model" }, reasoningEffort: null } })));
  });
});
