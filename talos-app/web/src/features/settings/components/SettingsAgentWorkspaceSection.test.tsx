// @vitest-environment jsdom

import { createElement, type PropsWithChildren } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import "@/shared/i18n/setup";
import { SettingsAgentWorkspaceSection } from "./SettingsAgentWorkspaceSection";

const getWorkspaceInfo = vi.fn();

vi.mock("@/features/connection", () => ({
  useConnectionSettings: () => ({
    connection: {
      authStatus: "authenticated",
      authRole: "user",
      userId: "alice",
    },
  }),
}));

vi.mock("@/features/chat", () => ({
  useConnectionKey: () => "settings-test-connection",
}));

vi.mock("@/shared/api/client", () => ({
  getTalosClient: () => ({
    workspaceDocs: {
      info: getWorkspaceInfo,
    },
  }),
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

describe("SettingsAgentWorkspaceSection", () => {
  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
  });

  it("shows server-provided workspace path content on the general tab", async () => {
    getWorkspaceInfo.mockResolvedValue({
      user_id: "alice",
      username: "Alice",
      base_workspace_dir: "/srv/talos/workspace-talos",
      user_workspace_dir: "/srv/talos/workspace-talos/users/alice",
    });

    render(<SettingsAgentWorkspaceSection />, { wrapper: createWrapper() });

    expect(screen.getByText("Workspace path")).toBeTruthy();
    await waitFor(() => {
      expect(screen.getByDisplayValue("/srv/talos/workspace-talos")).toBeTruthy();
      expect(screen.getByDisplayValue("/srv/talos/workspace-talos/users/alice")).toBeTruthy();
    });
    expect(screen.queryByText("2026-05-06")).toBeNull();
    expect(screen.queryByText("MEMORY.md")).toBeNull();
  });
});
