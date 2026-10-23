// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { StrictMode, useState } from "react";
import type { ReasoningLevel } from "@/shared/types/ui";

import { i18n } from "@/shared/i18n/setup";
import { resources } from "@/shared/i18n/resources";
import { renderComposer } from "../__tests__/ChatComposer.test-utils";
import type { ComposerModelOption } from "../model-selection";
import { ComposerModelPicker } from "./ComposerModelPicker";

const effortCapabilities = { reasoning: "effort", fast: false, default_fast: false, default_reasoning_effort: "high" } as const;
const models: ComposerModelOption[] = [
  { id: "llm-gateway:model", provider: "llm-gateway", providerName: "LLM Gateway", customProvider: true, name: "Qwen3 32B", reasoningEffort: "high", selectionCapabilities: effortCapabilities },
  { id: "local-inference:model", provider: "local-inference", providerName: "Local Inference", customProvider: true, name: "Qwen3.5 27B", reasoningEffort: "medium", selectionCapabilities: { ...effortCapabilities, reasoning: "toggle", default_reasoning_effort: null } },
];
const captures = new Set<number>();
const originalCapture = Object.getOwnPropertyDescriptor(HTMLElement.prototype, "setPointerCapture");
const originalHasCapture = Object.getOwnPropertyDescriptor(HTMLElement.prototype, "hasPointerCapture");
const originalRelease = Object.getOwnPropertyDescriptor(HTMLElement.prototype, "releasePointerCapture");

beforeEach(() => {
  vi.stubGlobal("ResizeObserver", class { observe() {} unobserve() {} disconnect() {} });
  captures.clear();
  Object.defineProperty(HTMLElement.prototype, "setPointerCapture", { configurable: true, value: (id: number) => captures.add(id) });
  Object.defineProperty(HTMLElement.prototype, "hasPointerCapture", { configurable: true, value: (id: number) => captures.has(id) });
  Object.defineProperty(HTMLElement.prototype, "releasePointerCapture", { configurable: true, value: (id: number) => captures.delete(id) });
});
afterEach(async () => {
  cleanup();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
  await i18n.changeLanguage("en");
  for (const [key, descriptor] of [["setPointerCapture", originalCapture], ["hasPointerCapture", originalHasCapture], ["releasePointerCapture", originalRelease]] as const) {
    if (descriptor) Object.defineProperty(HTMLElement.prototype, key, descriptor);
    else Reflect.deleteProperty(HTMLElement.prototype, key);
  }
});

describe("ComposerModelPicker", () => {
  it("browses Providers without committing and selects the complete model identity", () => {
    const onModelChange = vi.fn();
    renderComposer({ models, selectedModel: "llm-gateway:model", reasoningLevel: "high", onModelChange });
    fireEvent.click(screen.getByRole("button", { name: "Qwen3 32B · High" }));
    fireEvent.click(screen.getByRole("button", { name: "Select model: Qwen3 32B, Provider: LLM Gateway" }));
    fireEvent.click(screen.getByRole("button", { name: "Change Provider: LLM Gateway" }));
    fireEvent.click(screen.getByRole("button", { name: "Local Inference" }));
    expect(onModelChange).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "Qwen3.5 27B" })).toHaveAttribute("aria-pressed", "false");
    fireEvent.click(screen.getByRole("button", { name: "Qwen3.5 27B" }));
    expect(onModelChange).toHaveBeenCalledExactlyOnceWith("local-inference:model");
  });

  it("navigates Escape one level at a time and restores the trigger on close", async () => {
    renderComposer({ models, selectedModel: "llm-gateway:model", reasoningLevel: "high" });
    const trigger = screen.getByRole("button", { name: "Qwen3 32B · High" });
    fireEvent.click(trigger);
    fireEvent.click(screen.getByRole("button", { name: "Select model: Qwen3 32B, Provider: LLM Gateway" }));
    fireEvent.click(screen.getByRole("button", { name: "Change Provider: LLM Gateway" }));
    fireEvent.keyDown(document, { key: "Escape" });
    expect(screen.getByRole("button", { name: "Change Provider: LLM Gateway" })).toBeInTheDocument();
    fireEvent.keyDown(document, { key: "Escape" });
    expect(screen.getByRole("slider", { name: "Reasoning depth" })).toBeInTheDocument();
    fireEvent.keyDown(document, { key: "Escape" });
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "Model and reasoning settings" })).not.toBeInTheDocument());
    await waitFor(() => expect(trigger).toHaveFocus());
  });

  it("uses configured reset depth and commits keyboard changes without reacting to hover", () => {
    const onReasoningLevelChange = vi.fn();
    renderComposer({ models, selectedModel: "llm-gateway:model", reasoningLevel: "light", onReasoningLevelChange });
    fireEvent.click(screen.getByRole("button", { name: "Qwen3 32B · Light" }));
    const slider = screen.getByRole("slider", { name: "Reasoning depth" });
    fireEvent.pointerMove(slider, { pointerId: 1, clientX: 220 });
    expect(slider).toHaveAttribute("aria-valuetext", "Light");
    expect(onReasoningLevelChange).not.toHaveBeenCalled();
    fireEvent.keyDown(slider, { key: "ArrowRight" });
    expect(onReasoningLevelChange).toHaveBeenCalledExactlyOnceWith("medium");
    fireEvent.click(screen.getByRole("button", { name: "Reset reasoning depth to High" }));
    expect(onReasoningLevelChange).toHaveBeenLastCalledWith("high");
  });

  it("commits Home to Off and End to Extra High under StrictMode controlled updates", () => {
    const committed = vi.fn();
    function ControlledPicker() {
      const [effort, setEffort] = useState<ReasoningLevel>("medium");
      return <ComposerModelPicker models={models} selectedModel={models[0].id} reasoningLevel={effort} onReasoningLevelChange={(level) => { committed(level); setEffort(level); }} />;
    }
    render(<StrictMode><ControlledPicker /></StrictMode>);
    fireEvent.click(screen.getByRole("button", { name: "Qwen3 32B · Medium" }));
    const slider = screen.getByRole("slider", { name: "Reasoning depth" });
    slider.focus();
    fireEvent.keyDown(slider, { key: "End", code: "End" });
    expect(committed).toHaveBeenCalledExactlyOnceWith("extra_high");
    expect(slider).toHaveAttribute("aria-valuetext", "Extra High");
    fireEvent.keyDown(slider, { key: "Home", code: "Home" });
    expect(committed.mock.calls).toEqual([["extra_high"], ["none"]]);
    expect(slider).toHaveAttribute("aria-valuetext", "Off");
    expect(screen.getByRole("button", { name: "Qwen3 32B · Off" })).toBeInTheDocument();
    fireEvent.keyDown(slider, { key: "Home", code: "Home" });
    expect(committed).toHaveBeenCalledTimes(2);
    fireEvent.keyDown(slider, { key: "ArrowRight", code: "ArrowRight" });
    expect(committed).toHaveBeenLastCalledWith("light");
  });

  it("previews explicit dragging but persists only on pointer release", () => {
    const onReasoningLevelChange = vi.fn();
    renderComposer({ reasoningLevel: "light", onReasoningLevelChange });
    fireEvent.click(screen.getByRole("button", { name: "Model 1 · Light" }));
    const slider = screen.getByRole("slider", { name: "Reasoning depth" });
    const track = slider.closest<HTMLElement>("[data-effort]")!;
    vi.spyOn(track, "getBoundingClientRect").mockReturnValue({ x: 0, y: 0, top: 0, left: 0, right: 240, bottom: 24, width: 240, height: 24, toJSON() {} });
    fireEvent.pointerDown(track, { pointerId: 1, button: 0, clientX: 120 });
    fireEvent.pointerMove(track, { pointerId: 1, clientX: 240 });
    expect(onReasoningLevelChange).not.toHaveBeenCalled();
    fireEvent.pointerUp(track, { pointerId: 1, clientX: 240 });
    expect(onReasoningLevelChange).toHaveBeenCalledExactlyOnceWith("extra_high");
  });

  it("renders only Off/On for declared toggle capability and preserves its configured On effort", () => {
    const onReasoningLevelChange = vi.fn();
    renderComposer({ models, selectedModel: "local-inference:model", reasoningLevel: "none", onReasoningLevelChange });
    fireEvent.click(screen.getByRole("button", { name: "Qwen3.5 27B · Off" }));
    const slider = screen.getByRole("slider", { name: "Reasoning depth" });
    expect(slider).toHaveAttribute("aria-valuemax", "1");
    expect(slider).toHaveAttribute("aria-valuetext", "Off");
    fireEvent.keyDown(slider, { key: "End" });
    expect(onReasoningLevelChange).toHaveBeenCalledExactlyOnceWith("medium");
    expect(slider).toHaveAttribute("aria-valuetext", "On");
    expect(screen.getByRole("button", { name: "Reset reasoning depth to Off" })).toBeInTheDocument();
  });

  it("disables undeclared controls rather than guessing capabilities from names", () => {
    renderComposer({ models: [{ ...models[0], name: "Fast Reasoner", selectionCapabilities: { reasoning: "none", fast: false, default_fast: false, default_reasoning_effort: null } }], selectedModel: models[0].id });
    fireEvent.click(screen.getByRole("button", { name: "Fast Reasoner · Off" }));
    expect(screen.getByRole("slider", { name: "This model does not support reasoning" })).toHaveAttribute("aria-disabled", "true");
    const fast = screen.getByRole("button", { name: "Fast" });
    expect(fast).toHaveAttribute("aria-disabled", "true");
    fireEvent.click(fast);
    expect(fast).toHaveAttribute("aria-pressed", "false");
  });

  it("keeps Fast effects local and resets them to each selected model's configured default", () => {
    const onModelChange = vi.fn();
    const onReasoningLevelChange = vi.fn();
    const configuredModels = [{ ...models[0], selectionCapabilities: { ...effortCapabilities, fast: true, default_fast: true } }, models[1]];
    const show = (selectedModel: string) => <ComposerModelPicker models={configuredModels} selectedModel={selectedModel} reasoningLevel="light" onModelChange={onModelChange} onReasoningLevelChange={onReasoningLevelChange} />;
    const view = render(show(models[0].id));
    fireEvent.click(screen.getByRole("button", { name: "Fast UI On (speed adaptation pending), Qwen3 32B · Light" }));
    expect(screen.getByRole("button", { name: "Fast" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByText("UI state only. Speed adaptation is not connected yet.")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Fast" }));
    expect(screen.getByRole("button", { name: "Fast" })).toHaveAttribute("aria-pressed", "false");
    expect(screen.getByRole("button", { name: "Qwen3 32B · Light" })).toBeInTheDocument();
    expect(onModelChange).not.toHaveBeenCalled();
    expect(onReasoningLevelChange).not.toHaveBeenCalled();
    view.rerender(show(models[1].id));
    expect(screen.getByRole("button", { name: "Fast" })).toHaveAttribute("aria-pressed", "false");
    view.rerender(show(models[0].id));
    expect(screen.getByRole("button", { name: "Fast" })).toHaveAttribute("aria-pressed", "true");
    fireEvent.keyDown(document, { key: "Escape" });
    fireEvent.click(screen.getByRole("button", { name: "Fast UI On (speed adaptation pending), Qwen3 32B · Light" }));
    expect(screen.getByRole("button", { name: "Fast" })).toHaveAttribute("aria-pressed", "true");
  });

  it("preserves focus on passive updates and on an outside control after dismissal", async () => {
    const props = { models, selectedModel: models[0].id, reasoningLevel: "high" as const };
    const show = (level: "high" | "light") => <><input aria-label="Outside" /><ComposerModelPicker {...props} reasoningLevel={level} /></>;
    const view = render(show("high"));
    expect(document.activeElement).toBe(document.body);
    const outside = screen.getByRole("textbox", { name: "Outside" });
    outside.focus();
    view.rerender(show("light"));
    expect(outside).toHaveFocus();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Qwen3 32B · Light" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "Fast" })).toHaveFocus());
    outside.focus();
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    await waitFor(() => expect(outside).toHaveFocus());
  });

  it.each(["en", "zh-CN"] as const)("localizes controls, tooltips and navigation in %s", async (locale) => {
    await i18n.changeLanguage(locale);
    const chinese = locale === "zh-CN";
    expect(Object.keys(resources.en.chat.modelPicker).sort()).toEqual(Object.keys(resources["zh-CN"].chat.modelPicker).sort());
    renderComposer({ models, selectedModel: models[1].id, reasoningLevel: "none", onReasoningLevelChange: vi.fn() });
    fireEvent.click(screen.getByRole("button", { name: chinese ? "Qwen3.5 27B · 关闭" : "Qwen3.5 27B · Off" }));
    expect(screen.getByRole("dialog", { name: chinese ? "模型与思考设置" : "Model and reasoning settings" })).toBeInTheDocument();
    expect(screen.getByRole("slider", { name: chinese ? "思考深度" : "Reasoning depth" })).toHaveAttribute("aria-valuetext", chinese ? "关闭" : "Off");
    expect(screen.getByRole("button", { name: chinese ? "恢复默认思考深度：关闭" : "Reset reasoning depth to Off" })).toBeInTheDocument();
    expect(screen.getByText(chinese ? "当前服务未声明支持 Fast" : "This service has not declared Fast support")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: chinese ? "选择模型：Qwen3.5 27B，Provider：Local Inference" : "Select model: Qwen3.5 27B, Provider: Local Inference" }));
    expect(screen.getByRole("button", { name: chinese ? "返回" : "Back" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: chinese ? "切换 Provider：Local Inference" : "Change Provider: Local Inference" })).toBeInTheDocument();
  });
});
