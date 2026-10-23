import { beforeEach, describe, expect, it } from "vitest";

import "@/shared/i18n/setup";
import { i18n } from "@/shared/i18n/setup";
import {
  createDefaultProjects,
  describeProjectDirectory,
  resolveProjectView,
  hasRealSessionTitle,
  outputContentToText,
  UNBOUND_PROJECT_ID,
} from "@/shared/types/domain";

describe("domain helpers", () => {
  beforeEach(async () => {
    await i18n.changeLanguage("en");
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

    expect(resolveProjectView(projects, "missing-project")?.id).toBe(UNBOUND_PROJECT_ID);
    expect(resolveProjectView(projects, "project-1")?.id).toBe("project-1");
  });

  it("detects generated session titles separately from the default title", () => {
    expect(hasRealSessionTitle("Untitled Session")).toBe(false);
    expect(hasRealSessionTitle("  ")).toBe(false);
    expect(hasRealSessionTitle("Plan Talos Desktop Smoke")).toBe(true);
  });

  it("formats canonical tool content items without parsing JSON strings", () => {
    const rendered = outputContentToText([
      {
        type: "web_reference",
        title: "Example",
        url: "https://example.com",
        citation_id: "web-1",
        snippet: "Example snippet",
        source_domain: "example.com",
        published_at: null,
        provider_rank: 1,
      },
      {
        type: "search_results",
        query: "needle",
        matches: [
          {
            path: "src/lib.rs",
            line: 7,
            column: 3,
            text: "needle",
            before: [],
            after: [],
          },
        ],
        offset: 0,
        limit: 10,
        total_matches: 1,
        more_available: false,
        partial: false,
      },
      {
        type: "bash_output",
        session_id: null,
        status: "completed",
        exit_code: 0,
        stdout: "ok",
        stderr: "",
        truncated: false,
        elapsed_ms: null,
        outputLog: null,
      },
    ]);

    expect(rendered).toContain("Example");
    expect(rendered).toContain("https://example.com");
    expect(rendered).toContain("Example snippet");
    expect(rendered).toContain("src/lib.rs:7: needle");
    expect(rendered).toContain("ok");
  });
});
