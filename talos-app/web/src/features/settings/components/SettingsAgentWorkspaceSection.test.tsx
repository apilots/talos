// @vitest-environment jsdom

import { createElement, type PropsWithChildren } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { i18n } from "@/shared/i18n/setup";
import { SettingsPersonalizationSection } from "./SettingsPersonalizationSection";

vi.mock("@/shared/api/client", () => ({
  getTalosClient: () => ({ workspaceDocs: { info: async () => ({
    workspaceId: "team", rootPrefix: "/srv/talos/workspace-team", userWorkspaceDir: "/srv/talos/workspace-team/alice",
  }) } }),
}));

vi.mock("@/features/connection", () => ({
  useConnectionSettings: () => ({
    connection: {
      authStatus: "authenticated",
      authRole: "user",
      accountType: "Free",
      authToken: "",
      authUsername: "alice",
      userId: "alice",
    },
  }),
}));

vi.mock("@/features/chat", () => ({
  useConnectionKey: () => "settings-test-connection",
}));

vi.mock("@/features/workspace-docs", () => ({
  formatWorkspaceDocDate: () => "May 06, 2026",
  useWorkspaceDocsPage: () => ({
    docKinds: ["memory"],
    docLabels: { memory: "MEMORY.md" },
    selectedDocKind: "memory",
    setSelectedDocKind: vi.fn(),
    selectedDetail: {
      exists: true,
      path: "MEMORY.md",
      content: "Instructions",
      updated_at: Date.now(),
    },
    docDraft: "Instructions",
    setDocDraft: vi.fn(),
    saveCurrent: vi.fn(),
    isDirty: false,
    isLoading: false,
    isSaving: false,
    loadError: null,
    saveError: null,
  }),
}));

vi.mock("@/features/memory", () => ({
  formatMemoryDate: () => "May 06, 2026",
  useMemoryPage: () => ({
    query: "",
    setQuery: vi.fn(),
    files: [{ date: "2026-05-06" }],
    searchResults: [],
    searchResultDates: [],
    selectedDate: "2026-05-06",
    selectedDetail: {
      date: "2026-05-06",
      section_count: 1,
      content: "Memory content",
      version_token: "v1",
      updated_at: Date.now(),
    },
    editorContent: "Memory content",
    setEditorContent: vi.fn(),
    selectDate: vi.fn(),
    saveCurrent: vi.fn(),
    isDirty: false,
    isLoadingList: false,
    isLoadingDetail: false,
    isSearching: false,
    isSearchPending: false,
    isSaving: false,
    searchError: null,
    saveError: null,
  }),
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
    return createElement(QueryClientProvider, { client: queryClient }, children);
  };
}

describe("SettingsPersonalizationSection", () => {
  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
  });

  it("shows read-only actor workspace locations and localized labels", async () => {
    const { container } = render(<SettingsPersonalizationSection />, { wrapper: createWrapper() });
    await screen.findByText("/srv/talos/workspace-team/alice");
    expect(container.querySelector("dl")?.textContent).toMatchInlineSnapshot(`"Workspace IDteamStorage prefix/srv/talos/workspace-teamYour workspace path/srv/talos/workspace-team/alice"`);
    expect(container.querySelector("input")).toBeNull();
    await act(() => i18n.changeLanguage("zh-CN"));
    expect(screen.getByText("你的工作区路径")).toBeTruthy();
    await act(() => i18n.changeLanguage("en"));
  });

  it("switches between instructions and memory", () => {
    render(<SettingsPersonalizationSection />, { wrapper: createWrapper() });

    const instructionsTab = screen.getByRole("button", { name: "Instructions" });
    const memoryTab = screen.getByRole("button", { name: "Memory" });

    fireEvent.click(instructionsTab);
    expect(instructionsTab.getAttribute("aria-pressed")).toBe("true");
    expect(memoryTab.getAttribute("aria-pressed")).toBe("false");

    fireEvent.click(memoryTab);

    expect(instructionsTab.getAttribute("aria-pressed")).toBe("false");
    expect(memoryTab.getAttribute("aria-pressed")).toBe("true");
  });
});
