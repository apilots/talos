// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { AppProtocol } from "@talos/sdk-core";
import { MessageTimelineBlock } from "./MessageTimelineBlock";
import { SystemNotice } from "./SystemNotice";
import { i18n } from "@/shared/i18n/setup";

afterEach(async () => { cleanup(); await i18n.changeLanguage("en"); });

it("shows a failed history turn even without generated text and reacts to locale changes", async () => {
  const error: AppProtocol.ApiError = {
    code: AppProtocol.ErrorCode.ProviderUnavailable,
    message: 'Model "alpha" is not configured for provider "local".',
    localization: { type: "modelNotConfigured", provider: "local", model: "alpha" },
  };
  render(<MessageTimelineBlock block={{ id: "failed", kind: "timeline", items: [], error }}
    copiedLabel="Copied" copyLabel="Copy" helpfulLabel="Helpful" notHelpfulLabel="Not helpful" />);
  expect(screen.getByRole("alert").textContent).toBe(error.message);
  await act(() => i18n.changeLanguage("zh-CN"));
  expect(screen.getByRole("alert").textContent).toMatchInlineSnapshot('"提供方“local”未配置模型“alpha”。"');
});

it("keeps turn warnings in their original activity position and translates on render", async () => {
  const notice: AppProtocol.AgentSystemNotice = { level: "warning", content: { type: "promptWarning", data: {
    message: "Prompt bootstrap file is missing: IDENTITY.md",
    warning: { type: "bootstrapFileMissing", file: "identity" },
  } } };
  const { container } = render(<MessageTimelineBlock block={{ id: "success", kind: "timeline", items: [
    { itemId: "before", startedAtMs: 1, kind: "reasoning", phase: "thinking", status: "completed", text: "Before warning" },
    { itemId: "notice", startedAtMs: 2, kind: "system_notice", phase: "system", status: "completed", text: "", notice },
    { itemId: "after", startedAtMs: 3, kind: "reasoning", phase: "thinking", status: "completed", text: "After warning" },
  ] }} copiedLabel="Copied" copyLabel="Copy" helpfulLabel="Helpful" notHelpfulLabel="Not helpful" />);
  expect(container.querySelector("details")?.open).toBe(false);
  fireEvent.click(container.querySelector("summary")!);
  expect(container.querySelector("details")?.open).toBe(true);
  expect(Array.from(container.querySelectorAll("[data-process-step]")).map((item) => item.getAttribute("data-timeline-item-key")))
    .toEqual(["success:before", "success:notice", "success:after"]);
  const warning = screen.getByText("Prompt bootstrap file is missing: IDENTITY.md");
  expect(warning.closest("details")).toBeTruthy();
  expect(warning.closest("article")?.getAttribute("data-timeline-surface")).toBe("plain");
  await act(() => i18n.changeLanguage("zh-CN"));
  expect(container.querySelector('[data-timeline-item-key="success:notice"]')?.textContent)
    .toMatchInlineSnapshot('"系统缺少提示词引导文件：IDENTITY.md"');
});

it("keeps independent product notices visible with lightweight accessible presentation", async () => {
  const notice: AppProtocol.AgentSystemNotice = { level: "warning", content: { type: "error", data: {
    code: AppProtocol.ErrorCode.ProviderUnavailable, message: "Model is unavailable.",
    localization: { type: "modelNotConfigured", provider: "local", model: "alpha" },
  } } };
  const { container } = render(<SystemNotice notice={notice} />);
  expect(screen.getByRole("alert").getAttribute("data-notice-level")).toBe("warning");
  expect(container.querySelector("details")).toBeNull();
  expect(screen.getByRole("alert").className).toMatchInlineSnapshot('"flex items-start gap-2 px-1 py-2 text-sm leading-6 text-muted"');
  await act(() => i18n.changeLanguage("zh-CN"));
  expect(screen.getByRole("alert").textContent).toBe("提供方“local”未配置模型“alpha”。");
});
