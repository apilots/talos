// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import "@/shared/i18n/setup";
import { getComposerInput, getComposerValue, renderComposer, renderControlledComposer } from "./ChatComposer.test-utils";

describe("ChatComposer run state", () => {
  afterEach(cleanup);

  it("allows stopping an active response even when the composer is otherwise disabled", () => {
    const onCancel = vi.fn();
    renderComposer({ disabled: true, isStreaming: true, onCancel });

    const stopButton = screen.getByRole("button", { name: "Stop response" });
    expect(stopButton).not.toBeDisabled();
    expect(screen.getByRole("button", { name: /model 1.*medium/i })).toBeDisabled();

    fireEvent.click(stopButton);
    expect(onCancel).toHaveBeenCalledTimes(1);
  });

  it("disables the round send button while the composer is empty", () => {
    renderComposer({ draft: "" });

    expect(screen.getByRole("button", { name: "Send" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Send" })).toHaveAttribute("data-state", "disabled");
  });

  it("keeps the stop icon and adds a breathing state while cancellation is pending", () => {
    renderComposer({ isCancelling: true, isStreaming: true });

    const stopButton = screen.getByRole("button", { name: "Stop response" });
    expect(stopButton).toBeDisabled();
    expect(stopButton).toHaveAttribute("data-state", "cancelling");
    expect(stopButton).toHaveClass("animate-pulse");
    expect(stopButton.querySelector(".lucide-square")).toBeInTheDocument();
  });

  it("keeps the draft and blocks submission during context compaction", () => {
    const onCancel = vi.fn();
    const onSend = vi.fn();
    renderControlledComposer({
      commandResponse: {
        kind: "compact",
        stage: "summarizing",
        state: "running",
      },
      initialDraft: "keep this draft",
      onCancel,
      onSend,
    });

    expect(screen.getByRole("status")).toHaveTextContent("Compacting context");
    expect(screen.getByRole("status")).toHaveTextContent("Summarizing earlier conversation");
    fireEvent.keyDown(getComposerInput(), { key: "Enter" });
    expect(onSend).not.toHaveBeenCalled();
    expect(getComposerValue()).toBe("keep this draft");

    fireEvent.click(screen.getByRole("button", { name: "Stop response" }));
    expect(onCancel).toHaveBeenCalledTimes(1);
  });
});

describe("ChatComposer context usage", () => {
  afterEach(cleanup);

  it("shows the session context percentage and usage details", () => {
    renderComposer({
      contextUsage: {
        usedTokens: 56,
        contextWindowTokens: 100,
        effectivePromptBudgetTokens: 80,
        measuredAt: 1,
      },
      sessionTotalTokens: 114_500,
    });

    const indicator = screen.getByTestId("composer-context-usage");
    expect(indicator).toHaveTextContent("56.0%");
    expect(indicator).toHaveAttribute("aria-label", "Context usage: 56.0%");
    const tooltip = screen.getByRole("tooltip");
    expect(indicator).toHaveAttribute("aria-describedby", tooltip.id);
    expect(tooltip).toHaveTextContent("56 / 100 tokens");
    expect(tooltip).toHaveTextContent("44 tokens");
    expect(tooltip).toHaveTextContent("80 tokens");
    expect(tooltip).toHaveTextContent("114.5K tokens");
  });

});
