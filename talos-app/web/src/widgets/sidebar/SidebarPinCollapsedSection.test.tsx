// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { afterEach, expect, it, vi } from "vitest";

import "@/shared/i18n/setup";
import { Sidebar } from "./Sidebar";
import type { SidebarSessionItem } from "./SidebarSessionRow";

afterEach(cleanup);

it.each(["project", "Projects", "Chats"])("recovers focus when unpinning into collapsed %s", (destination) => {
  const projectSession = destination !== "Chats";
  const chat: SidebarSessionItem = {
    id: "chat-1", label: "Review changes", status: "running", model: "test-model",
    projectId: projectSession ? "project-1" : null, createdAt: 1, lastActivityAt: 1,
  };
  const props = {
    connection: {
      authStatus: "authenticated" as const, authRole: "user" as const,
      accountType: "Pro" as const, authUsername: "user", userId: "user-1",
    },
    currentPath: "/",
    onCreateProjectThread: vi.fn(), onCreateThread: vi.fn(), onLogout: vi.fn(),
    onDeleteSession: vi.fn(), onRemoveProject: vi.fn(), onRenameProject: vi.fn(),
    onSelectThread: vi.fn(), onTogglePinSession: vi.fn(),
  };
  const view = (pinned: boolean) => (
    <MemoryRouter>
      <Sidebar
        {...props}
        chats={!projectSession && !pinned ? [chat] : []}
        pinnedSessions={pinned ? [{ ...chat, pinned }] : []}
        projectGroups={projectSession ? [{
          project: {
            id: "project-1", name: "Project", directory: "/tmp/project", origin: "server",
            createdAt: 1, updatedAt: 1, sessionCount: 1,
          },
          sessions: pinned ? [] : [chat],
        }] : []}
      />
    </MemoryRouter>
  );
  const { rerender } = render(view(true));
  if (destination !== "project") {
    fireEvent.click(screen.getByRole("button", { name: destination }));
  }
  const source = screen.getByRole("button", { name: "Unpin Review changes" });
  source.focus();
  fireEvent.click(source);
  rerender(view(false));

  expect(document.activeElement).toBe(screen.getByRole("button", {
    name: destination === "project" ? "Project" : destination,
  }));
});
