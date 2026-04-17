import { beforeEach, describe, expect, it } from "vitest";

import "@/shared/i18n/setup";
import { i18n } from "@/shared/i18n/setup";
import {
  createDefaultProjects,
  getApprovalModeHint,
  getApprovalModeLabel,
  getProjectDisplayName,
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

  it("localizes the default workspace display name", async () => {
    expect(getProjectDisplayName({ type: "server_visible_path", path: __TALOS_DEFAULT_PROJECT_ROOT__ })).toBe(
      "Default workspace",
    );

    await i18n.changeLanguage("zh-CN");

    expect(getProjectDisplayName({ type: "server_visible_path", path: __TALOS_DEFAULT_PROJECT_ROOT__ })).toBe(
      "默认工作区",
    );
  });

  it("creates a default project keyed to the workspace root", async () => {
    const projects = createDefaultProjects();

    expect(projects).toHaveLength(1);
    expect(projects[0]?.root).toEqual({
      type: "server_visible_path",
      path: __TALOS_DEFAULT_PROJECT_ROOT__,
    });
  });
});
