// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import "@/shared/i18n/setup";
import { i18n } from "@/shared/i18n/setup";
import { ProjectSessionsPage } from "./ProjectSessionsPage";

const navigateMock = vi.fn();
const useParamsMock = vi.fn();
const useProjectSessionsPageStateMock = vi.fn();

vi.mock("react-router-dom", async () => {
  const actual = await vi.importActual<typeof import("react-router-dom")>("react-router-dom");
  return {
    ...actual,
    useNavigate: () => navigateMock,
    useParams: () => useParamsMock(),
  };
});

vi.mock("@/features/projects", () => ({
  ProjectSessionRow: ({ title }: { title: string }) => <div>{title}</div>,
  useProjectSessionsPageState: () => useProjectSessionsPageStateMock(),
}));

function buildProjectSessionsState() {
  return {
    notFound: false,
    project: {
      id: "project-1",
      name: "Alpha",
      directory: "/srv/alpha",
      createdAt: 1,
      updatedAt: 10,
      sessionCount: 0,
    },
    query: "",
    sessionRows: [],
    sessionsQuery: {
      isLoading: false,
      isError: false,
      error: null,
      refetch: vi.fn(),
    },
    setQuery: vi.fn(),
  };
}

describe("ProjectSessionsPage", () => {
  beforeEach(async () => {
    vi.clearAllMocks();
    await i18n.changeLanguage("zh-CN");
    useParamsMock.mockReturnValue({ projectId: "project-1" });
    useProjectSessionsPageStateMock.mockReturnValue(buildProjectSessionsState());
  });

  it("does not render a delete action on the detail page", () => {
    render(<ProjectSessionsPage />);

    expect(screen.queryByRole("button", { name: "删除" })).toBeNull();
  });
});
