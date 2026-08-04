import { afterEach, describe, expect, it } from "vitest";
import { AppProtocol, TalosApiError } from "@talos/sdk-browser";

import { apiErrorMessage } from "./apiErrorMessage";
import { resources } from "./resources";
import { i18n } from "./setup";

afterEach(async () => {
  await i18n.changeLanguage("en");
});

describe("apiErrorMessage", () => {
  it("covers every generated error code in supported locales", () => {
    const codes = Object.values(AppProtocol.ErrorCode);

    for (const code of codes) {
      expect(resources.en.errors).toHaveProperty(code);
      expect(resources["zh-CN"].errors).toHaveProperty(code);
    }
  });

  it("uses the error code translation with the server message as fallback", async () => {
    await i18n.changeLanguage("zh-CN");
    const error = new TalosApiError({
      code: AppProtocol.ErrorCode.InternalError,
      message: "An internal error occurred",
    });

    expect(apiErrorMessage(error)).toBe("发生内部错误");
    expect(
      apiErrorMessage({
        code: AppProtocol.ErrorCode.InvalidRetryRequest,
        message: "Invalid retry request",
      }),
    ).toBe("重试请求无效");
  });
});
