// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi, beforeEach, afterEach } from "vitest";

import "@/shared/i18n/setup";
import { i18n } from "@/shared/i18n/setup";
import { ProjectsPage } from "./ProjectsPage";

const navigateMock = vi.fn();
const useProjectsPageStateMock = vi.fn();
const useCreateProjectMutationMock = vi.fn();
const useDeleteProjectMutationMock = vi.fn();

vi.mock("react-router-dom", async () => {
  const actual = await vi.importActual<typeof import("react-router-dom")>("react-router-dom");
  return {
    ...actual,
    useNavigate: () => navigateMock,
  };
});

vi.mock("@/features/chat", () => ({
  useCreateProjectMutation: () => useCreateProjectMutationMock(),
  useDeleteProjectMutation: () => useDeleteProjectMutationMock(),
}));

vi.mock("@/features/projects", async () => {
  const actual = await vi.importActual<typeof import("@/features/projects")>("@/features/projects");
  return {
    ...actual,
    useProjectsPageState: () => useProjectsPageStateMock(),
  };
});

function buildProjectsPageState(sessionCount = 0) {
  return {
    filteredProjects: [
      {
        id: "project-1",
        name: "Alpha",
        directory: "/srv/alpha",
        createdAt: 1,
        updatedAt: 10,
        sessionCount,
      },
    ],
    projectsQuery: {
      isLoading: false,
      isError: false,
      error: null,
      refetch: vi.fn(),
    },
    query: "",
    setQuery: vi.fn(),
    setSort: vi.fn(),
    sort: "activity",
  };
}

describe("ProjectsPage", () => {
  beforeEach(async () => {
    vi.clearAllMocks();
    await i18n.changeLanguage("zh-CN");
    useCreateProjectMutationMock.mockReturnValue({
      isPending: false,
      mutateAsync: vi.fn(),
    });
  });

  afterEach(() => {
    cleanup();
  });

  it("disables delete for projects with bound sessions", () => {
    useProjectsPageStateMock.mockReturnValue(buildProjectsPageState(2));
    useDeleteProjectMutationMock.mockReturnValue({
      isPending: false,
      mutateAsync: vi.fn(),
      variables: undefined,
    });

    render(<ProjectsPage />);

    expect(screen.getByRole("button", { name: "删除" })).toBeDisabled();
  });

  it("shows a friendly conflict message when delete is rejected by the server", async () => {
    const mutateAsync = vi.fn().mockRejectedValue(new Error("PROJECT_DELETE_CONFLICT"));
    vi.stubGlobal("confirm", vi.fn(() => true));
    useProjectsPageStateMock.mockReturnValue(buildProjectsPageState(0));
    useDeleteProjectMutationMock.mockReturnValue({
      isPending: false,
      mutateAsync,
      variables: undefined,
    });

    render(<ProjectsPage />);

    fireEvent.click(screen.getByRole("button", { name: "删除" }));

    expect(await screen.findByText("该项目下仍有已绑定会话，请先解绑或删除这些会话。")).toBeInTheDocument();
    expect(mutateAsync).toHaveBeenCalledWith("project-1");
  });
});
