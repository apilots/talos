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

function session(
  id: string,
  label: string,
  projectId?: string | null,
  pinned = false,
): SidebarSessionItem {
  return {
    id,
    label,
    status: "done",
    model: "gpt-5.5",
    projectId,
    pinned,
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
      sessionCount: 1,
    },
    sessions: [session("project-session-1", "Review API design", "project-talos")],
  },
];

function renderSidebar(overrides: Partial<ComponentProps<typeof Sidebar>> = {}) {
  const props: ComponentProps<typeof Sidebar> = {
    chats: [session("chat-1", "Compare JSON-GRPC and WebSocket", null)],
    connection: {
      authStatus: "authenticated",
      authRole: "user",
      accountType: "Pro",
      authUsername: "user",
      userId: "user-1",
    },
    currentPath: "/",
    onCreateProjectThread: vi.fn(),
    onCreateThread: vi.fn(),
    onLogout: vi.fn(),
    onDeleteSession: vi.fn(),
    onTogglePinSession: vi.fn(),
    onRemoveProject: vi.fn(),
    onRenameProject: vi.fn(),
    onSelectThread: vi.fn(),
    projectGroups,
    ...overrides,
  };

  return {
    ...render(
      <MemoryRouter initialEntries={["/"]}>
        <Sidebar {...props} />
      </MemoryRouter>,
    ),
    props,
  };
}

describe("Sidebar pinned sessions", () => {
  afterEach(() => {
    cleanup();
  });

  it("hides the Pinned section when there are no pinned sessions", () => {
    renderSidebar({ pinnedSessions: [] });

    expect(screen.queryByText("Pinned")).toBeNull();
  });

  it("renders Pinned above Projects when pinned sessions exist, including running state", () => {
    const runningPinned = session("pinned-running", "Review Talos PR 211", null, true);
    runningPinned.status = "running";
    const { container } = renderSidebar({
      pinnedSessions: [
        session("pinned-1", "Pinned analysis", "project-talos", true),
        runningPinned,
      ],
    });

    const pinnedHeading = screen.getByText("Pinned");
    const projectsHeading = screen.getByText("Projects");
    expect(
      pinnedHeading.compareDocumentPosition(projectsHeading) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
    expect(screen.getByText("Pinned analysis")).toBeTruthy();
    expect(
      screen.getByRole("status", { name: "Running Review Talos PR 211" }),
    ).toBeTruthy();
    expect(container.querySelector("section[aria-label='Pinned']")).not.toBeNull();
  });

  it("toggles pin and unpin without selecting the session row", () => {
    const { props } = renderSidebar({
      pinnedSessions: [session("pinned-1", "Pinned chat", null, true)],
      chats: [session("chat-1", "Compare JSON-GRPC and WebSocket", null, false)],
      projectGroups: [],
    });

    fireEvent.click(screen.getByRole("button", { name: "Pin Compare JSON-GRPC and WebSocket" }));
    expect(props.onTogglePinSession).toHaveBeenCalledWith("chat-1", true);
    expect(props.onSelectThread).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "Unpin Pinned chat" }));
    expect(props.onTogglePinSession).toHaveBeenCalledWith("pinned-1", false);
    expect(props.onSelectThread).not.toHaveBeenCalled();
  });

  it("displays an error alert without throwing when toggling pin fails", async () => {
    const onTogglePinSession = vi.fn().mockRejectedValue(new Error("Network error"));
    renderSidebar({
      chats: [session("chat-1", "Compare JSON-GRPC and WebSocket", null, false)],
      onTogglePinSession,
      projectGroups: [],
    });

    fireEvent.click(screen.getByRole("button", { name: "Pin Compare JSON-GRPC and WebSocket" }));

    await waitFor(() => {
      expect(screen.getByRole("alert").textContent).toBe("Network error");
    });
  });

  it("restores focus to the moved pin button after keyboard toggle", async () => {
    const chat = session("chat-1", "Compare JSON-GRPC and WebSocket", null, false);
    let resolvePin!: () => void;
    const onTogglePinSession = vi.fn(
      () =>
        new Promise<void>((resolve) => {
          resolvePin = resolve;
        }),
    );
    const { rerender } = render(
      <MemoryRouter initialEntries={["/"]}>
        <Sidebar
          chats={[chat]}
          connection={{
            authStatus: "authenticated",
            authRole: "user",
            accountType: "Pro",
            authUsername: "user",
            userId: "user-1",
          }}
          currentPath="/"
          onCreateProjectThread={vi.fn()}
          onCreateThread={vi.fn()}
          onDeleteSession={vi.fn()}
          onLogout={vi.fn()}
          onRemoveProject={vi.fn()}
          onRenameProject={vi.fn()}
          onSelectThread={vi.fn()}
          onTogglePinSession={onTogglePinSession}
          pinnedSessions={[]}
          projectGroups={[]}
        />
      </MemoryRouter>,
    );

    const pinButton = screen.getByRole("button", { name: "Pin Compare JSON-GRPC and WebSocket" });
    pinButton.focus();
    fireEvent.click(pinButton);

    rerender(
      <MemoryRouter initialEntries={["/"]}>
        <Sidebar
          chats={[]}
          connection={{
            authStatus: "authenticated",
            authRole: "user",
            accountType: "Pro",
            authUsername: "user",
            userId: "user-1",
          }}
          currentPath="/"
          onCreateProjectThread={vi.fn()}
          onCreateThread={vi.fn()}
          onDeleteSession={vi.fn()}
          onLogout={vi.fn()}
          onRemoveProject={vi.fn()}
          onRenameProject={vi.fn()}
          onSelectThread={vi.fn()}
          onTogglePinSession={onTogglePinSession}
          pinnedSessions={[{ ...chat, pinned: true }]}
          projectGroups={[]}
        />
      </MemoryRouter>,
    );

    resolvePin();
    await waitFor(() => {
      const movedButton = screen.getByRole("button", {
        name: "Unpin Compare JSON-GRPC and WebSocket",
      });
      expect(document.activeElement).toBe(movedButton);
    });
  });
});
