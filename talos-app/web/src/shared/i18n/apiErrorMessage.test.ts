import { afterEach, describe, expect, it } from "vitest";
import { AppProtocol, TalosApiError } from "@talos/sdk-browser";

import { apiErrorMessage, userVisibleErrorMessage } from "./apiErrorMessage";
import { i18n } from "./setup";

afterEach(async () => {
  await i18n.changeLanguage("en");
});

describe("apiErrorMessage", () => {
  it("keeps empty and unknown local exceptions visible", () => {
    for (const error of [new Error(""), new Error("  "), undefined]) {
      expect(userVisibleErrorMessage(error, "Failed to load the session.")).toBe("Failed to load the session.");
    }
  });
  it("never replaces a specific reason with a generic code translation", async () => {
    await i18n.changeLanguage("zh-CN");
    const error = new TalosApiError({
      code: AppProtocol.ErrorCode.InternalError, localization: null,
      message: "An internal error occurred",
    });

    expect(apiErrorMessage(error)).toBe("An internal error occurred");
    expect(
      apiErrorMessage({
        code: AppProtocol.ErrorCode.InvalidRetryRequest, localization: null,
        message: "Invalid retry request",
      }),
    ).toBe("Invalid retry request");
    expect(
      apiErrorMessage({
        code: AppProtocol.ErrorCode.MessageQueueBusy, localization: null,
        message: "Message queue message is busy",
      }),
    ).toBe("Message queue message is busy");
  });

  it("renders a typed reason in the current locale without mutating stored data", async () => {
    const error: AppProtocol.ApiError = {
      code: AppProtocol.ErrorCode.ProviderUnavailable,
      message: 'Model "alpha" is not configured for provider "local".',
      localization: { type: "modelNotConfigured", provider: "local", model: "alpha" },
    };
    await i18n.changeLanguage("zh-CN");
    expect(apiErrorMessage(error)).toBe('提供方“local”未配置模型“alpha”。');
    await i18n.changeLanguage("en");
    expect(apiErrorMessage(error)).toBe(error.message);
  });

  it("preserves English when a descriptor translation is absent", async () => {
    const error: AppProtocol.ApiError = {
      code: AppProtocol.ErrorCode.AttachmentLimitExceeded,
      message: "This request contains 5 attachments; the limit is 3.",
      localization: { type: "attachmentCountExceeded", limit: 3, actual: 5 },
    };
    const translations = i18n.getResourceBundle("en", "errors");
    i18n.removeResourceBundle("en", "errors");
    try {
      expect(apiErrorMessage(error)).toBe(error.message);
    } finally {
      i18n.addResourceBundle("en", "errors", translations);
    }
  });
});
