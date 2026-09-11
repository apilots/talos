import type { AppProtocol } from "@talos/sdk-core";
import { afterEach, expect, it } from "vitest";
import { i18n } from "./setup";
import { systemNoticeMessage } from "./systemNoticeMessage";

afterEach(() => i18n.changeLanguage("en").then(() => undefined));

it("translates every prompt warning variant while keeping filenames and counts", async () => {
  const warnings: AppProtocol.PromptWarning[] = [
    { type: "bootstrapFileMissing", file: "identity" },
    { type: "bootstrapFileTruncated", file: "soul", injectedChars: 10, rawChars: 30 },
    { type: "bootstrapFileDropped", file: "agents" },
    { type: "dynamicSectionsDropped", count: 2 },
  ];
  const render = () => warnings.map((warning) => systemNoticeMessage({ level: "warning", content: {
    type: "promptWarning", data: { message: "Reviewed English fallback", warning },
  } }));
  await i18n.changeLanguage("en");
  expect(render()).toMatchInlineSnapshot(`
    [
      "Prompt bootstrap file is missing: IDENTITY.md",
      "Prompt bootstrap file was truncated: SOUL.md (10 of 30 characters)",
      "Prompt bootstrap file was dropped: AGENTS.md",
      "Prompt dynamic sections were dropped: 2",
    ]
  `);
  await i18n.changeLanguage("zh-CN");
  expect(render()).toMatchInlineSnapshot(`
    [
      "缺少提示词引导文件：IDENTITY.md",
      "提示词引导文件已截断：SOUL.md（保留 10 / 30 个字符）",
      "提示词引导文件未纳入上下文：AGENTS.md",
      "有 2 个动态提示词片段未纳入上下文。",
    ]
  `);
});
