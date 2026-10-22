// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { ComponentProps } from "react";
import { MemoryRouter } from "react-router-dom";
import { afterEach, describe, expect, it, vi } from "vitest";

import "@/shared/i18n/setup";
import { Sidebar } from "./Sidebar";
import type { SidebarProjectGroup } from "./SidebarProjectSection";
import type { SidebarSessionItem } from "./SidebarSessionRow";

const now = 1_725_000_000;

function session(id: string, label: string, projectId?: string | null): SidebarSessionItem {
  return {
    id,
    label,
    status: "done",
    model: "gpt-5.5",
    projectId,
    createdAt: now,
    lastActivityAt: now,
  };
}

const projectGroups: SidebarProjectGroup[] = [
  {
    project: {
      id: "project-talos",
      name: "talos",
      directory: "/Users/test/talos",
      createdAt: now,
      updatedAt: now,
      origin: "server",
      sessionCount: 2,
    },
    sessions: [
      session("project-session-1", "Review API design", "project-talos"),
      session("project-session-2", "Package server module", "project-talos"),
    ],
  },
];

function createSessions(count: number, projectId?: string | null) {
  return Array.from({ length: count }, (_, index) =>
    session(
      `${projectId ?? "chat"}-${index + 1}`,
      `${projectId ? "Project" : "Chat"} session ${index + 1}`,
      projectId,
    ),
  );
}

function renderSidebar(overrides: Partial<ComponentProps<typeof Sidebar>> = {}, routerPath = "/") {
  const props = {
    chats: [session("chat-1", "Compare JSON-GRPC and WebSocket", null)],
    connection: {
      authStatus: "authenticated" as const,
      authRole: "user" as const,
      accountType: "Pro" as const,
      authUsername: "user",
      userId: "user-1",
    },
    currentPath: "/sessions/project-session-1",
    onCreateProjectThread: vi.fn(),
    onCreateThread: vi.fn(),
    onLogout: vi.fn(),
    onDeleteSession: vi.fn(),
    onRemoveProject: vi.fn(),
    onRenameProject: vi.fn(),
    onSelectThread: vi.fn(),
    projectGroups,
    ...overrides,
  };

  return {
    ...render(
      <MemoryRouter initialEntries={[routerPath]}>
        <Sidebar {...props} />
      </MemoryRouter>,
    ),
    props,
  };
}

describe("Sidebar", () => {
  afterEach(() => {
    cleanup();
  });

  it("renders the main navigation and grouped session regions", () => {
    renderSidebar();

    expect(screen.getByText("Talos")).toBeTruthy();
    expect(screen.getByText("New chat")).toBeTruthy();
    expect(screen.getByText("Memories")).toBeTruthy();
    expect(screen.getByText("Plugins")).toBeTruthy();
    expect(screen.getByText("Files")).toBeTruthy();
    expect(screen.getByText("Projects")).toBeTruthy();
    expect(screen.getByText("Chats")).toBeTruthy();
    expect(screen.getByText("talos")).toBeTruthy();
    expect(screen.queryByText("Review API design")).toBeNull();
    expect(screen.getByText("Compare JSON-GRPC and WebSocket")).toBeTruthy();
  });

  it("uses a neutral surface for the selected chat", () => {
    renderSidebar({
      currentPath: "/sessions/chat-1",
      projectGroups: [],
    });

    const selectedChat = screen.getByText("Compare JSON-GRPC and WebSocket").closest("button");
    expect(selectedChat).not.toBeNull();
    expect(selectedChat?.getAttribute("data-selected")).toBe("true");
  });

  it("shows a motion-safe spinner only while a session is running", () => {
    const runningSession = session("chat-running", "Analyze Rust compilation");
    runningSession.status = "running";
    renderSidebar({
      chats: [runningSession, session("chat-done", "Review completed analysis")],
      currentPath: "/sessions/chat-running",
      projectGroups: [],
    });

    const runningStatus = screen.getByRole("status", {
      name: "Running Analyze Rust compilation",
    });
    expect(runningStatus.querySelector("svg")?.classList).toContain("motion-safe:animate-spin");
    expect(runningStatus.querySelector("svg")?.classList).toContain(
      "motion-safe:[animation-duration:1.6s]",
    );
    expect(runningStatus).toMatchSnapshot();
    expect(screen.queryByRole("status", { name: "Running Review completed analysis" })).toBeNull();
  });

  it("applies the selected surface directly to the active primary navigation route", () => {
    renderSidebar({ projectGroups: [] }, "/plugins");

    const plugins = screen.getByText("Plugins").closest("a");
    expect(plugins?.getAttribute("aria-current")).toBe("page");
  });

  it("toggles project rows and keeps chat rows wired to navigation callbacks", () => {
    const { props } = renderSidebar();

    fireEvent.click(screen.getByText("talos"));
    expect(screen.getByText("Review API design")).toBeTruthy();

    fireEvent.click(screen.getByText("Review API design"));
    expect(props.onSelectThread).toHaveBeenCalledWith("project-session-1");

    fireEvent.click(screen.getByText("talos"));
    expect(screen.queryByText("Review API design")).toBeNull();

    fireEvent.click(screen.getByText("Compare JSON-GRPC and WebSocket"));
    expect(props.onSelectThread).toHaveBeenCalledWith("chat-1");
  });

  it("shows pending deletion feedback while keeping the delete action disabled", () => {
    renderSidebar({ sessionActionPending: true, projectGroups: [] });
    fireEvent.click(screen.getByRole("button", { name: "Delete Compare JSON-GRPC and WebSocket" }));
    expect(screen.getByRole("button", { name: "Deleting…" }).hasAttribute("disabled")).toBe(true);
  });

  it("deletes the selected session after confirmation without selecting it again", async () => {
    const { props } = renderSidebar({
      currentPath: "/sessions/chat-1",
      projectGroups: [],
    });

    fireEvent.click(screen.getByRole("button", { name: "Delete Compare JSON-GRPC and WebSocket" }));
    expect(props.onSelectThread).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: /^Delete$/ }));

    await waitFor(() => {
      expect(props.onDeleteSession).toHaveBeenCalledWith("chat-1");
    });
  });

  it("allows multiple projects to expand independently from the project row", () => {
    renderSidebar({
      currentPath: "/",
      projectGroups: [
        ...projectGroups,
        {
          project: {
            id: "project-codex",
            name: "codex",
            directory: "/Users/test/codex",
            createdAt: now,
            updatedAt: now,
            origin: "server",
            sessionCount: 1,
          },
          sessions: [session("project-session-3", "Explain sandbox design", "project-codex")],
        },
      ],
    });

    expect(screen.queryByText("Review API design")).toBeNull();
    expect(screen.queryByText("Explain sandbox design")).toBeNull();

    fireEvent.click(screen.getByText("talos"));
    fireEvent.click(screen.getByText("codex"));

    expect(screen.getByText("Review API design")).toBeTruthy();
    expect(screen.getByText("Explain sandbox design")).toBeTruthy();

    fireEvent.click(screen.getByText("talos"));

    expect(screen.queryByText("Review API design")).toBeNull();
    expect(screen.getByText("Explain sandbox design")).toBeTruthy();
  });

  it("keeps a selected project session full width without selecting its parent project", () => {
    renderSidebar({ currentPath: "/sessions/project-session-1" });

    fireEvent.click(screen.getByText("talos"));

    const projectRow = screen.getByText("talos").closest('[data-sidebar-project-row="true"]');
    const sessionButton = screen.getByText("Review API design").closest("button");

    expect(projectRow).not.toBeNull();
    expect(projectRow?.getAttribute("data-selected")).toBeNull();
    expect(sessionButton?.getAttribute("data-selected")).toBe("true");
  });

  it("opens and closes project actions without leaving the menu behind", async () => {
    renderSidebar();

    fireEvent.click(screen.getByRole("button", { name: "Project actions" }));

    expect(screen.getByRole("button", { name: "Rename project" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Remove" })).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Pin project" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Reveal in Finder" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Create permanent worktree" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Archive chats" })).toBeNull();

    fireEvent.pointerDown(document.body);

    await waitFor(() => {
      expect(screen.queryByRole("button", { name: "Rename project" })).toBeNull();
    });
  });

  it("creates a new project chat from the project row edit action", () => {
    const { props } = renderSidebar();

    fireEvent.click(screen.getByRole("button", { name: "New chat in project" }));

    expect(props.onCreateProjectThread).toHaveBeenCalledWith("project-talos");
  });

  it("renames a project from the project actions dialog", async () => {
    const { props } = renderSidebar();

    fireEvent.click(screen.getByRole("button", { name: "Project actions" }));
    fireEvent.click(screen.getByRole("button", { name: "Rename project" }));
    fireEvent.change(screen.getByRole("textbox"), { target: { value: "talos-next" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => {
      expect(props.onRenameProject).toHaveBeenCalledWith("project-talos", "talos-next");
    });
  });

  it("removes a project from the project actions dialog", async () => {
    const { props } = renderSidebar();

    fireEvent.click(screen.getByRole("button", { name: "Project actions" }));
    fireEvent.click(screen.getByRole("button", { name: "Remove" }));
    fireEvent.click(screen.getByRole("button", { name: /^Remove$/ }));

    await waitFor(() => {
      expect(props.onRemoveProject).toHaveBeenCalledWith("project-talos");
    });
  });

  it("shows settings from the account menu", () => {
    renderSidebar();

    expect(screen.getByText("Pro")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: /user/i }));

    expect(screen.getByRole("link", { name: /^Settings/ })).toBeTruthy();
    expect(screen.getByRole("link", { name: /^Profile$/ })).toBeTruthy();
    expect(screen.getByText("Log out")).toBeTruthy();
  });

  it("expands chat rows from Show more when more than ten chats exist", () => {
    renderSidebar({
      chats: createSessions(12, null),
      currentPath: "/sessions/chat-1",
      projectGroups: [],
    });

    expect(screen.getByText("Chat session 10")).toBeTruthy();
    expect(screen.queryByText("Chat session 11")).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Show more" }));

    expect(screen.getByText("Chat session 11")).toBeTruthy();
    expect(screen.getByText("Chat session 12")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Show more" })).toBeNull();
  });

  it("expands project session rows from Show more when more than five project sessions exist", () => {
    const projectId = "project-talos";
    renderSidebar({
      currentPath: "/sessions/project-talos-1",
      projectGroups: [
        {
          project: {
            id: projectId,
            name: "talos",
            directory: "/Users/test/talos",
            createdAt: now,
            updatedAt: now,
            origin: "server",
            sessionCount: 7,
          },
          sessions: createSessions(7, projectId),
        },
      ],
    });

    fireEvent.click(screen.getByText("talos"));

    expect(screen.getByText("Project session 5")).toBeTruthy();
    expect(screen.queryByText("Project session 6")).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Show more" }));

    expect(screen.getByText("Project session 6")).toBeTruthy();
    expect(screen.getByText("Project session 7")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Show more" })).toBeNull();
  });

});
