import { beforeEach, describe, expect, it } from "vitest";

import "@/shared/i18n/setup";
import { i18n } from "@/shared/i18n/setup";
import {
  approvalModeToPolicy,
  createDefaultProjects,
  describeProjectDirectory,
  getApprovalModeHint,
  getApprovalModeLabel,
  resolveProjectRecord,
  getSessionApprovalMode,
  hasRealSessionTitle,
  UNBOUND_PROJECT_ID,
} from "@/shared/types/domain";

describe("domain helpers", () => {
  beforeEach(async () => {
    await i18n.changeLanguage("en");
  });

  it("localizes approval labels and hints", async () => {
    expect(getApprovalModeLabel("needs_approval")).toBe("Default permissions");
    expect(getApprovalModeHint("skip")).toBe("Allow all tools without per-call confirmation.");
    expect(approvalModeToPolicy("needs_approval")).toBe("always");
    expect(approvalModeToPolicy("skip")).toBe("never");

    await i18n.changeLanguage("zh-CN");

    expect(getApprovalModeLabel("needs_approval")).toBe("默认权限");
    expect(getApprovalModeHint("skip")).toBe("所有工具可直接执行，无需逐次确认。");
  });

  it("derives approval mode from persisted session configuration", () => {
    expect(
      getSessionApprovalMode({
        conf: {
          tools: {
            approval_policy: "always",
          },
        },
      } as never),
    ).toBe("needs_approval");

    expect(
      getSessionApprovalMode({
        conf: {
          tools: {
            approval_policy: "on_request",
          },
        },
      } as never),
    ).toBe("skip");
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

  it("falls back to the first available project when the active id is stale", () => {
    const projects = [
      ...createDefaultProjects(),
      {
        id: "project-1",
        name: "alpha",
        directory: "/workspace/alpha",
        createdAt: 1,
        updatedAt: 2,
        origin: "server" as const,
        sessionCount: 1,
      },
    ];

    expect(resolveProjectRecord(projects, "missing-project")?.id).toBe(UNBOUND_PROJECT_ID);
    expect(resolveProjectRecord(projects, "project-1")?.id).toBe("project-1");
  });

  it("detects generated session titles separately from the default title", () => {
    expect(hasRealSessionTitle("Untitled Session")).toBe(false);
    expect(hasRealSessionTitle("  ")).toBe(false);
    expect(hasRealSessionTitle("Plan Talos Desktop Smoke")).toBe(true);
  });
});
