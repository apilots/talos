// @vitest-environment jsdom
import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { AppProtocol } from "@talos/sdk-core";
import { MessageTimelineBlock } from "./MessageTimelineBlock";
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

it("keeps structured warnings outside collapsed activity and translates them at render time", async () => {
  render(<MessageTimelineBlock block={{ id: "success", kind: "timeline", items: [
    { itemId: "notice", startedAtMs: 1, kind: "system_notice", phase: "system", status: "completed", text: "", notice: {
      level: "warning", content: { type: "error", data: {
        code: AppProtocol.ErrorCode.ProviderUnavailable, message: "Model is unavailable.",
        localization: { type: "modelNotConfigured", provider: "local", model: "alpha" },
      } },
    } },
  ] }} copiedLabel="Copied" copyLabel="Copy" helpfulLabel="Helpful" notHelpfulLabel="Not helpful" />);
  expect(screen.getByRole("alert").getAttribute("data-notice-level")).toBe("warning");
  await act(() => i18n.changeLanguage("zh-CN"));
  expect(screen.getByRole("alert").textContent).toMatchInlineSnapshot('"提供方“local”未配置模型“alpha”。"');
});
