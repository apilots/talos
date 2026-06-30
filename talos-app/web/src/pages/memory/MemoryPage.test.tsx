// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import "@/shared/i18n/setup";
import { i18n } from "@/shared/i18n/setup";
import { MemoryPage } from "./MemoryPage";

const useMemoryPageMock = vi.fn();
const useConnectionSettingsMock = vi.fn();

vi.mock("@/features/memory", () => ({
  formatMemoryDate: () => "Apr 21, 2026",
  useMemoryPage: () => useMemoryPageMock(),
}));

vi.mock("@/features/connection", () => ({
  useConnectionSettings: () => useConnectionSettingsMock(),
}));

vi.mock("@/widgets/message-list/MarkdownRenderer", () => ({
  MarkdownRenderer: ({ text }: { text: string }) => <div data-testid="markdown-preview">{text}</div>,
}));

function buildMemoryState() {
  return {
    query: "",
    setQuery: vi.fn(),
    files: [
      {
        date: "2026-04-21",
      },
    ],
    searchResults: [],
    searchResultDates: [],
    selectedDate: "2026-04-21",
    selectedDetail: {
      date: "2026-04-21",
      section_count: 1,
      content: "# Shared memory",
      version_token: "token-1",
      updated_at: 1776393847462,
    },
    editorContent: "# Shared memory",
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
  };
}

describe("MemoryPage", () => {
  it("allows authenticated users to switch into edit mode", async () => {
    await i18n.changeLanguage("zh-CN");
    useConnectionSettingsMock.mockReturnValue({
      connection: {
        authStatus: "authenticated",
        authRole: "user",
      },
    });
    useMemoryPageMock.mockReturnValue(buildMemoryState());

    render(<MemoryPage />);

    fireEvent.click(screen.getByRole("button", { name: "编辑" }));

    expect(screen.getByRole("button", { name: "保存" })).toBeInTheDocument();
    expect(screen.getByDisplayValue("# Shared memory")).toBeInTheDocument();
  });
});
