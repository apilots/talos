// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import type { PropsWithChildren } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { SkillPluginsManagementView } from "./SkillPluginsManagementView";

const apiMocks = vi.hoisted(() => ({
  listSkills: vi.fn(),
  listManagedSkillPlugins: vi.fn(),
  searchSkillPlugins: vi.fn(),
}));

vi.mock("@/features/plugins/api", () => ({
  disablePlugin: vi.fn(),
  enablePlugin: vi.fn(),
  installPlugin: vi.fn(),
  listManagedSkillPlugins: apiMocks.listManagedSkillPlugins,
  listSkills: apiMocks.listSkills,
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

  it("shows read-only third-party skills as extension skills without plugin operations", async () => {
    apiMocks.listSkills.mockResolvedValue({
      data: [
        {
          name: "find-skills",
          description: "Find installable skills.",
          short_description: null,
          root: "/Users/xujiang/.agents/skills/find-skills",
          path: "/Users/xujiang/.agents/skills/find-skills/SKILL.md",
          scope: "configured",
          source_plugin: null,
          source_install: null,
          enabled: true,
          read_only: true,
          metadata: null,
        },
        {
          name: "algorithmic-art",
          description: "Create algorithmic art.",
          short_description: null,
          root: "/Users/xujiang/.codex/skills/algorithmic-art",
          path: "/Users/xujiang/.codex/skills/algorithmic-art/SKILL.md",
          scope: "configured",
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
    apiMocks.listManagedSkillPlugins.mockResolvedValue({ plugins: [], warnings: [] });
    apiMocks.searchSkillPlugins.mockResolvedValue({ plugins: [], warnings: [] });

    render(<SkillPluginsManagementView query="" refreshVersion={0} />, {
      wrapper: createWrapper(),
    });

    await waitFor(() => {
      expect(screen.getByText("Extension skills")).toBeInTheDocument();
      expect(screen.getByText("Find Skills")).toBeInTheDocument();
      expect(screen.getByText("Algorithmic Art")).toBeInTheDocument();
    });

    expect(screen.queryByText("Marketplace Writer")).not.toBeInTheDocument();
    expect(screen.getAllByText("Read-only")).toHaveLength(2);
    expect(screen.getAllByText("Extension")).toHaveLength(2);
    expect(screen.queryByTitle("Uninstall")).not.toBeInTheDocument();
    expect(screen.queryByTitle("Disable")).not.toBeInTheDocument();
  });
});
