// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import "@/shared/i18n/setup";
import { renderComposer } from "./ChatComposer.test-utils";

describe("ChatComposer model menu", () => {
  afterEach(cleanup);

  it("keeps a long model name accessible and selectable when the trigger is truncated", () => {
    const name = "DeepSeek V4 Flash Vision (Experimental)";
    const onReasoningLevelChange = vi.fn();
    renderComposer({
      models: [{ id: "deepseek:vision", name, provider: "deepseek" }],
      selectedModel: "deepseek:vision",
      reasoningLevel: "light",
      onReasoningLevelChange,
    });

    const trigger = screen.getByRole("button", { name: `${name} · Light` });
    expect(trigger).toHaveAttribute("title", `${name} · Light`);
    trigger.focus();
    fireEvent.click(trigger);
    expect(screen.getByRole("button", { name: `Model: ${name}` })).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "Reasoning: Light" }));
    fireEvent.click(screen.getByRole("button", { name: "Medium" }));
    expect(onReasoningLevelChange).toHaveBeenCalledWith("medium");
  });

  it("shows only a disabled none control when reasoning is unsupported", () => {
    renderComposer({ reasoningEnabled: false, reasoningLevel: "none" });

    fireEvent.click(screen.getByRole("button", { name: /model 1.*none/i }));
    expect(screen.getByRole("button", { name: /reasoning: none/i })).toBeDisabled();
    expect(screen.queryByText("Light")).not.toBeInTheDocument();
  });

  it("selects the first configured model and filters models when the provider changes", () => {
    const onModelChange = vi.fn();
    renderComposer({
      models: [
        { id: "deepseek:deepseek-v4-flash", name: "DeepSeek V4 Flash", provider: "deepseek" },
        { id: "deepseek:deepseek-v4-pro", name: "DeepSeek V4 Pro", provider: "deepseek" },
        { id: "anthropic:claude-opus", name: "Claude Opus", provider: "anthropic" },
        { id: "anthropic:claude-sonnet", name: "Claude Sonnet", provider: "anthropic" },
      ],
      onModelChange,
      selectedModel: "deepseek:deepseek-v4-flash",
    });

    fireEvent.click(screen.getByRole("button", { name: /deepseek v4 flash.*medium/i }));
    expect(screen.queryByText("Speed")).not.toBeInTheDocument();
    expect(screen.queryByText("Advanced")).not.toBeInTheDocument();
    fireEvent.mouseEnter(screen.getByRole("button", { name: "Provider: deepseek" }));
    expect(screen.getByRole("button", { name: "Provider: deepseek" })).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByRole("button", { name: "Model: DeepSeek V4 Flash" })).toHaveAttribute("aria-expanded", "false");
    fireEvent.click(screen.getByRole("button", { name: "anthropic" }));

    expect(onModelChange).toHaveBeenCalledWith("anthropic:claude-opus");
    expect(screen.queryByRole("button", { name: "Provider: anthropic" })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /claude opus.*medium/i }));
    expect(screen.getByRole("button", { name: "Model: Claude Opus" })).toBeInTheDocument();

    fireEvent.mouseEnter(screen.getByRole("button", { name: "Model: Claude Opus" }));
    expect(screen.getByRole("button", { name: "Provider: anthropic" })).toHaveAttribute("aria-expanded", "false");
    expect(screen.getByRole("button", { name: "Model: Claude Opus" })).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByRole("button", { name: "Claude Sonnet" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "DeepSeek V4 Pro" })).not.toBeInTheDocument();
  });

  it("keeps the selected model when the current provider is selected again", () => {
    const onModelChange = vi.fn();
    renderComposer({
      models: [
        { id: "deepseek:deepseek-v4-flash", name: "DeepSeek V4 Flash", provider: "deepseek" },
        { id: "deepseek:deepseek-v4-pro", name: "DeepSeek V4 Pro", provider: "deepseek" },
        { id: "anthropic:claude-opus", name: "Claude Opus", provider: "anthropic" },
      ],
      onModelChange,
      selectedModel: "deepseek:deepseek-v4-pro",
    });

    fireEvent.click(screen.getByRole("button", { name: /deepseek v4 pro.*medium/i }));
    fireEvent.mouseEnter(screen.getByRole("button", { name: "Provider: deepseek" }));
    fireEvent.click(screen.getByRole("button", { name: "deepseek" }));

    expect(onModelChange).not.toHaveBeenCalled();
    expect(screen.queryByRole("button", { name: "Provider: deepseek" })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /deepseek v4 pro.*medium/i }));
    expect(screen.getByRole("button", { name: "Model: DeepSeek V4 Pro" })).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Model: DeepSeek V4 Pro" }));
    fireEvent.click(screen.getByRole("button", { name: "DeepSeek V4 Pro" }));
    expect(onModelChange).not.toHaveBeenCalled();
    expect(screen.queryByRole("button", { name: "Model: DeepSeek V4 Pro" })).not.toBeInTheDocument();
  });

  it("changes reasoning effort from the combined model menu", () => {
    const onReasoningLevelChange = vi.fn();
    renderComposer({ onReasoningLevelChange, reasoningLevel: "medium" });

    fireEvent.click(screen.getByRole("button", { name: /model 1.*medium/i }));
    fireEvent.mouseEnter(screen.getByRole("button", { name: "Reasoning: Medium" }));
    fireEvent.click(screen.getByRole("button", { name: "Reasoning: Medium" }));
    fireEvent.click(screen.getByRole("button", { name: "High" }));

    expect(onReasoningLevelChange).toHaveBeenCalledWith("high");
    expect(screen.queryByRole("button", { name: "Reasoning: Medium" })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /model 1.*medium/i }));
    fireEvent.mouseEnter(screen.getByRole("button", { name: "Reasoning: Medium" }));
    fireEvent.click(screen.getByRole("button", { name: "Medium" }));
    expect(onReasoningLevelChange).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("button", { name: "Reasoning: Medium" })).not.toBeInTheDocument();
  });
});
