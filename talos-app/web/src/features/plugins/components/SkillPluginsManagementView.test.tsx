// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { PropsWithChildren } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { SkillPluginsManagementView } from "./SkillPluginsManagementView";

const apiMocks = vi.hoisted(() => ({
  listSkills: vi.fn(),
  listManagedSkillPlugins: vi.fn(),
  readLocalSkill: vi.fn(),
  searchSkillPlugins: vi.fn(),
}));

vi.mock("@/features/plugins/api", () => ({
  disablePlugin: vi.fn(),
  enablePlugin: vi.fn(),
  installPlugin: vi.fn(),
  listManagedSkillPlugins: apiMocks.listManagedSkillPlugins,
  listSkills: apiMocks.listSkills,
  readLocalSkill: apiMocks.readLocalSkill,
  searchSkillPlugins: apiMocks.searchSkillPlugins,
  uninstallPlugin: vi.fn(),
  updatePlugin: vi.fn(),
}));

function createWrapper() {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: {
        retry: false,
      },
    },
  });

  return function Wrapper({ children }: PropsWithChildren) {
    return <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>;
  };
}

describe("SkillPluginsManagementView", () => {
  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
  });

  it("shows read-only local skills without plugin operations and opens SKILL.md preview", async () => {
    const findSkills = {
      name: "find-skills",
      description: "Find installable skills.",
      short_description: null,
      root: "/Users/xujiang/.agents/skills/find-skills",
      path: "/Users/xujiang/.agents/skills/find-skills/SKILL.md",
      scope: "external",
      source_plugin: null,
      source_install: null,
      enabled: true,
      read_only: true,
      metadata: null,
    };
    apiMocks.listSkills.mockResolvedValue({
      data: [
        findSkills,
        {
          name: "algorithmic-art",
          description: "Create algorithmic art.",
          short_description: null,
          root: "/Users/xujiang/.codex/skills/algorithmic-art",
          path: "/Users/xujiang/.codex/skills/algorithmic-art/SKILL.md",
          scope: "external",
          source_plugin: null,
          source_install: null,
          enabled: true,
          read_only: true,
          metadata: null,
        },
        {
          name: "marketplace-writer",
          description: "Installed marketplace skill.",
          short_description: null,
          root: "/Users/xujiang/.talos/plugins/installs/writer/skills/marketplace-writer",
          path: "/Users/xujiang/.talos/plugins/installs/writer/skills/marketplace-writer/SKILL.md",
          scope: "plugin",
          source_plugin: "writer",
          source_install: "install-writer",
          enabled: true,
          read_only: true,
          metadata: null,
        },
      ],
      errors: [],
    });
    apiMocks.readLocalSkill.mockResolvedValue({
      document: {
        summary: findSkills,
        contents: "---\nname: find-skills\n---\n\n# Find Skills\n\nPreview body.",
      },
    });
    apiMocks.listManagedSkillPlugins.mockResolvedValue({ plugins: [], warnings: [] });
    apiMocks.searchSkillPlugins.mockResolvedValue({ plugins: [], warnings: [] });

    render(<SkillPluginsManagementView query="" refreshVersion={0} />, {
      wrapper: createWrapper(),
    });

    await waitFor(() => {
      expect(screen.getByRole("heading", { name: "Local" })).toBeInTheDocument();
      expect(screen.getByText("Find Skills")).toBeInTheDocument();
      expect(screen.getByText("Algorithmic Art")).toBeInTheDocument();
    });

    expect(screen.queryByText("Marketplace Writer")).not.toBeInTheDocument();
    expect(screen.getAllByText("Read-only")).toHaveLength(2);
    expect(screen.getAllByText("Local")).toHaveLength(3);
    expect(screen.queryByTitle("Uninstall")).not.toBeInTheDocument();
    expect(screen.queryByTitle("Disable")).not.toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Marketplace" })).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Find Skills" }));

    await waitFor(() => {
      expect(apiMocks.readLocalSkill).toHaveBeenCalledWith(findSkills);
      expect(screen.getByText("Preview body.")).toBeInTheDocument();
    });
  });
});
