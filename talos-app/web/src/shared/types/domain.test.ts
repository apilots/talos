import { beforeEach, describe, expect, it } from "vitest";

import "@/shared/i18n/setup";
import { i18n } from "@/shared/i18n/setup";
import {
  createDefaultProjects,
  describeProjectDirectory,
  getApprovalModeHint,
  getApprovalModeLabel,
  UNBOUND_PROJECT_ID,
} from "@/shared/types/domain";

describe("domain helpers", () => {
  beforeEach(async () => {
    await i18n.changeLanguage("en");
  });

  it("localizes approval labels and hints", async () => {
    expect(getApprovalModeLabel("needs_approval")).toBe("Default permissions");
    expect(getApprovalModeHint("skip")).toBe("Allow safe tools without per-call confirmation.");

    await i18n.changeLanguage("zh-CN");

    expect(getApprovalModeLabel("needs_approval")).toBe("默认权限");
    expect(getApprovalModeHint("skip")).toBe("安全工具可直接执行，无需逐次确认。");
  });

  it("localizes the unbound project display name", async () => {
    expect(describeProjectDirectory(undefined)).toBe("No project");

    await i18n.changeLanguage("zh-CN");

    expect(describeProjectDirectory(undefined)).toBe("不绑定项目");
  });

  it("creates an unbound project option by default", async () => {
    const projects = createDefaultProjects();

    expect(projects).toHaveLength(1);
    expect(projects[0]?.id).toBe(UNBOUND_PROJECT_ID);
    expect(projects[0]?.directory).toBeNull();
  });
});
