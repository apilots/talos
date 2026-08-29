// @vitest-environment jsdom

import { createElement, type PropsWithChildren } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import "@/shared/i18n/setup";
import { SettingsPersonalizationSection } from "./SettingsPersonalizationSection";

vi.mock("@/features/connection", () => ({
  useConnectionSettings: () => ({
    connection: {
      authStatus: "authenticated",
      authRole: "user",
      accountType: "Free",
      authToken: "",
      authUsername: "alice",
      userId: "alice",
      runtimeMode: "local",
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

  it("does not expose workspace paths", () => {
    render(<SettingsPersonalizationSection />, { wrapper: createWrapper() });

    expect(screen.queryByText("Workspace path")).toBeNull();
    expect(screen.queryByDisplayValue("/srv/talos/workspace-talos")).toBeNull();
    expect(screen.getAllByText("MEMORY.md").length).toBeGreaterThan(0);
  });

  it("switches between instructions and memory", () => {
    render(<SettingsPersonalizationSection />, { wrapper: createWrapper() });

    const instructionsTab = screen.getByRole("button", { name: "Instructions" });
    const memoryTab = screen.getByRole("button", { name: "Memory" });

    expect(instructionsTab.getAttribute("aria-pressed")).toBe("true");
    expect(memoryTab.getAttribute("aria-pressed")).toBe("false");

    fireEvent.click(memoryTab);

    expect(instructionsTab.getAttribute("aria-pressed")).toBe("false");
    expect(memoryTab.getAttribute("aria-pressed")).toBe("true");
  });
});
