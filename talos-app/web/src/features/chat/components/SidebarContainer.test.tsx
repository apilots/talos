// @vitest-environment jsdom

import { cleanup, render, waitFor } from "@testing-library/react";
import type { ComponentProps } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { MemoryRouter } from "react-router-dom";

import "@/shared/i18n/setup";
import { SidebarContainer } from "./SidebarContainer";
import type { Sidebar } from "@/widgets/sidebar/Sidebar";

const mocks = vi.hoisted(() => ({
  sidebarProps: undefined as ComponentProps<typeof Sidebar> | undefined,
  deleteProject: vi.fn(),
  updateProjectName: vi.fn(),
  deleteSession: vi.fn(),
  deleteSessionAlias: vi.fn(),
  resetStream: vi.fn(),
  setSessionStatus: vi.fn(),
}));

vi.mock("@/widgets/sidebar/Sidebar", () => ({
  Sidebar: (props: ComponentProps<typeof Sidebar>) => {
    mocks.sidebarProps = props;
    return <div data-testid="sidebar" />;
  },
}));

vi.mock("../hooks/useChatQueries", () => ({
  useDeleteSessionMutation: () => ({
    isPending: false,
    mutateAsync: mocks.deleteSession,
  }),
  useDeleteProjectMutation: () => ({
    isPending: false,
    mutateAsync: mocks.deleteProject,
  }),
  useProjectsView: () => ({
    projects: [
      {
        id: "project-known",
        name: "Known",
        directory: "/tmp/known",
        createdAt: 1,
        updatedAt: 1,
        origin: "server",
        sessionCount: 1,
      },
    ],
    setActiveProjectId: vi.fn(),
  }),
  useSessionsQuery: () => ({
    data: [
      {
        id: "session-known-project",
        title: "Known project session",
        projectId: "project-known",
        modelSelection: {
          modelRef: { provider: "mock", id: "model-1" },
          reasoningEffort: null,
        },
        createdAt: 1,
        lastActivity: 3,
      },
      {
        id: "session-missing-project",
        title: "Missing project session",
        projectId: "project-missing",
        modelSelection: {
          modelRef: { provider: "mock", id: "model-1" },
          reasoningEffort: null,
        },
        createdAt: 1,
        lastActivity: 2,
      },
      {
        id: "session-chat",
        title: "Chat session",
        projectId: null,
        modelSelection: {
          modelRef: { provider: "mock", id: "model-1" },
          reasoningEffort: null,
        },
        createdAt: 1,
        lastActivity: 1,
      },
    ],
  }),
  useUpdateProjectNameMutation: () => ({
    isPending: false,
    mutateAsync: mocks.updateProjectName,
  }),
}));

vi.mock("@/features/auth", () => ({
  useLogout: () => vi.fn(),
  logoutUser: vi.fn(),
}));

vi.mock("@/shared/store/app-store", () => ({
  useAppStore: (selector: (state: Record<string, unknown>) => unknown) =>
    selector({
      aliases: {},
      connection: {
        authStatus: "authenticated",
        authRole: "user",
        accountType: "Pro",
        authUsername: "user",
        userId: "user",
        authToken: "",
      },
      sessionAliases: {},
      deleteSessionAlias: mocks.deleteSessionAlias,
      resetStream: mocks.resetStream,
      setSessionStatus: mocks.setSessionStatus,
      setConnection: vi.fn(),
      setSidebarOpen: vi.fn(),
      streams: {},
      transientStatuses: {},
    }),
}));

describe("SidebarContainer", () => {
  afterEach(() => {
    cleanup();
    mocks.sidebarProps = undefined;
  });

  it("keeps sessions visible when their project is missing from the project list", () => {
    render(
      <MemoryRouter initialEntries={["/"]}>
        <SidebarContainer />
      </MemoryRouter>,
    );

    expect(mocks.sidebarProps?.projectGroups).toHaveLength(1);
    expect(mocks.sidebarProps?.projectGroups[0]?.sessions.map((session) => session.id)).toEqual([
      "session-known-project",
    ]);
    expect(mocks.sidebarProps?.chats.map((session) => session.id)).toEqual([
      "session-missing-project",
      "session-chat",
    ]);
  });

  it("deletes the active session and clears its local state", async () => {
    render(
      <MemoryRouter initialEntries={["/sessions/session-chat"]}>
        <SidebarContainer />
      </MemoryRouter>,
    );

    await mocks.sidebarProps?.onDeleteSession("session-chat");

    await waitFor(() => {
      expect(mocks.deleteSession).toHaveBeenCalledWith("session-chat");
      expect(mocks.deleteSessionAlias).toHaveBeenCalledWith("session-chat");
      expect(mocks.resetStream).toHaveBeenCalledWith("session-chat");
      expect(mocks.setSessionStatus).toHaveBeenCalledWith("session-chat", undefined);
    });
  });
});
