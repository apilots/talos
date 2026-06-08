// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import "@/shared/i18n/setup";
import { i18n } from "@/shared/i18n/setup";
import { WorkspaceDocsPage } from "./WorkspaceDocsPage";

const useWorkspaceDocsPageMock = vi.fn();
const useConnectionSettingsMock = vi.fn();

vi.mock("@/features/workspace-docs", () => ({
  formatWorkspaceDocDate: () => "Apr 21, 2026",
  useWorkspaceDocsPage: () => useWorkspaceDocsPageMock(),
}));

vi.mock("@/features/connection", () => ({
  useConnectionSettings: () => useConnectionSettingsMock(),
}));

vi.mock("@/widgets/message-list/MarkdownRenderer", () => ({
  MarkdownRenderer: ({ text }: { text: string }) => <div data-testid="markdown-preview">{text}</div>,
}));

function buildWorkspaceDocsState() {
  return {
    docKinds: ["memory", "identity"],
    docLabels: {
      memory: "MEMORY.md",
      identity: "IDENTITY.md",
    },
    selectedDocKind: "memory",
    setSelectedDocKind: vi.fn(),
    selectedDetail: {
      path: "MEMORY.md",
      exists: true,
      content: "# Shared workspace doc",
      updated_at: 1776393847462,
    },
    docDraft: "# Shared workspace doc",
    setDocDraft: vi.fn(),
    saveCurrent: vi.fn(),
    isDirty: false,
    isLoading: false,
    isSaving: false,
    loadError: null,
    saveError: null,
  };
}

describe("WorkspaceDocsPage", () => {
  it("allows authenticated users to switch workspace docs into edit mode", async () => {
    await i18n.changeLanguage("zh-CN");
    useConnectionSettingsMock.mockReturnValue({
      connection: {
        authStatus: "authenticated",
        authRole: "user",
      },
    });
    useWorkspaceDocsPageMock.mockReturnValue(buildWorkspaceDocsState());

    render(<WorkspaceDocsPage />);

    fireEvent.click(screen.getByRole("button", { name: "编辑" }));

    expect(screen.getByRole("button", { name: "保存" })).toBeInTheDocument();
    expect(screen.getByDisplayValue("# Shared workspace doc")).toBeInTheDocument();
  });
});
