// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import "@/shared/i18n/setup";
import { ComposerCommandResponse } from "./ComposerCommandResponse";

const contextUsage = {
  usedTokens: 219_000,
  contextWindowTokens: 258_000,
  effectivePromptBudgetTokens: 240_000,
  measuredAt: 1,
};

describe("ComposerCommandResponse", () => {
  afterEach(cleanup);

  it("renders a live status snapshot and dismisses it", () => {
    const onDismiss = vi.fn();
    render(
      <ComposerCommandResponse
        contextUsage={contextUsage}
        onDismiss={onDismiss}
        response={{ kind: "status" }}
        status={{
          model: "DeepSeek V4 Flash",
          project: "talos / main",
          reasoningLevel: "high",
          runtimeMode: "local",
          sessionId: "01a01fa7-eb47-7563-be54-3dffd461dc3b",
          state: "ready",
        }}
      />,
    );

    expect(screen.getByRole("region", { name: "Session status" })).toHaveTextContent("DeepSeek V4 Flash");
    expect(screen.getByRole("region", { name: "Session status" })).toHaveTextContent("talos / main");
    expect(screen.getByRole("region", { name: "Session status" })).toHaveTextContent("84.9% used");
    expect(screen.getByRole("region", { name: "Session status" })).toHaveTextContent("219K / 258K tokens");

    fireEvent.click(screen.getByRole("button", { name: "Close" }));
    expect(onDismiss).toHaveBeenCalledTimes(1);
  });

  it("renders the live backend compaction stage", () => {
    render(
      <ComposerCommandResponse
        onDismiss={vi.fn()}
        response={{ kind: "compact", stage: "summarizing", state: "running" }}
      />,
    );

    expect(screen.getByRole("status")).toHaveTextContent("Compacting context");
    expect(screen.getByRole("status")).toHaveTextContent(
      "Summarizing earlier conversation while preserving recent work",
    );
    expect(screen.getByRole("status")).toHaveTextContent("Summarizing");
    expect(screen.queryByRole("progressbar")).not.toBeInTheDocument();
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
  });

  it("renders the outcome-specific terminal state", () => {
    render(
      <ComposerCommandResponse
        onDismiss={vi.fn()}
        response={{ kind: "compact", outcome: "no_changes", state: "success" }}
      />,
    );

    expect(screen.getByRole("status")).toHaveTextContent("Context already compact");
    expect(screen.getByRole("status")).toHaveTextContent("No new session history needed");
  });

  it("renders concise clear and model results", () => {
    const { rerender } = render(
      <ComposerCommandResponse
        onDismiss={vi.fn()}
        response={{ kind: "clear", state: "success" }}
      />,
    );
    expect(screen.getByRole("status")).toHaveTextContent("Session context cleared");

    rerender(
      <ComposerCommandResponse
        onDismiss={vi.fn()}
        response={{
          kind: "model",
          model: "GPT-5.6 Sol",
          reasoningLevel: "medium",
          state: "success",
        }}
      />,
    );
    expect(screen.getByRole("status")).toHaveTextContent("GPT-5.6 Sol · Medium");
  });
});
