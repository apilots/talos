// @vitest-environment jsdom

import { act, renderHook, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { ModelListResponse, ModelSelection } from "@talos/sdk-browser";

import { useConfiguredSessionModelSelection } from "./useConfiguredSessionModelSelection";

describe("useConfiguredSessionModelSelection", () => {
  it("uses and persists the configured default when the session model is unavailable", async () => {
    const defaultSelection: ModelSelection = {
      modelRef: { provider: "deepseek", id: "deepseek-v4-flash" },
      reasoningEffort: "light",
    };
    const catalog = {
      defaultModelSelection: defaultSelection,
      models: [
        {
          id: "deepseek-v4-flash",
          name: "DeepSeek V4 Flash",
          provider: "deepseek",
          reasoning: true,
          reasoning_effort: "light",
        },
      ],
    } as ModelListResponse;
    const updateSelection = vi.fn();
    const { result } = renderHook(() =>
      useConfiguredSessionModelSelection({
        catalog,
        selection: {
          modelRef: { provider: "removed", id: "removed-model" },
          reasoningEffort: "high",
        },
        sessionId: "session-1",
        updateSelection,
      }),
    );

    expect(result.current).toEqual(defaultSelection);
    await waitFor(() => expect(updateSelection).toHaveBeenCalledWith(defaultSelection));
  });

  it("repairs the same unavailable selection only once across optimistic updates", async () => {
    const defaultSelection: ModelSelection = {
      modelRef: { provider: "deepseek", id: "deepseek-v4-flash" },
      reasoningEffort: "light",
    };
    const unavailableSelection: ModelSelection = {
      modelRef: { provider: "removed", id: "removed-model" },
      reasoningEffort: "high",
    };
    const catalog = {
      defaultModelSelection: defaultSelection,
      models: [
        {
          id: "deepseek-v4-flash",
          name: "DeepSeek V4 Flash",
          provider: "deepseek",
          reasoning: true,
          reasoning_effort: "light",
        },
      ],
    } as ModelListResponse;
    const updateSelection = vi.fn();
    const selection = { current: unavailableSelection };
    const { rerender } = renderHook(() =>
      useConfiguredSessionModelSelection({
        catalog,
        selection: selection.current,
        sessionId: "session-1",
        updateSelection,
      }),
    );

    await waitFor(() => expect(updateSelection).toHaveBeenCalledTimes(1));

    act(() => {
      selection.current = defaultSelection;
    });
    rerender();
    act(() => {
      selection.current = unavailableSelection;
    });
    rerender();

    expect(updateSelection).toHaveBeenCalledTimes(1);
  });
});
