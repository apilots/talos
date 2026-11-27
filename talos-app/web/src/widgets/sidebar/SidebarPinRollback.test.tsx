// @vitest-environment jsdom

import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { afterEach, expect, it, vi } from "vitest";

import "@/shared/i18n/setup";
import { Sidebar } from "./Sidebar";
import type { SidebarSessionItem } from "./SidebarSessionRow";

afterEach(cleanup);

it.each([false, true])("preserves keyboard focus when a pin rollback starts pinned=%s", async (initialPinned) => {
  const chat: SidebarSessionItem = {
    id: "chat-1", label: "Review changes", status: "running", model: "test-model",
    pinned: initialPinned, createdAt: 1, lastActivityAt: 1,
  };
  let rejectPin!: (error: Error) => void;
  let pinRequest!: Promise<void>;
  const props = {
    connection: {
      authStatus: "authenticated" as const, authRole: "user" as const,
      accountType: "Pro" as const, authUsername: "user", userId: "user-1",
    },
    currentPath: "/", projectGroups: [],
    onCreateProjectThread: vi.fn(), onCreateThread: vi.fn(), onLogout: vi.fn(),
    onDeleteSession: vi.fn(), onRemoveProject: vi.fn(), onRenameProject: vi.fn(),
    onSelectThread: vi.fn(),
    onTogglePinSession: () => {
      pinRequest = new Promise<void>((_resolve, reject) => { rejectPin = reject; });
      return pinRequest;
    },
  };
  const view = (pinned: boolean, pending: boolean) => (
    <MemoryRouter>
      <Sidebar
        {...props}
        chats={pinned ? [] : [{ ...chat, pinned }]}
        pinnedSessions={pinned ? [{ ...chat, pinned }] : []}
        pendingPinSessionIds={new Set(pending ? [chat.id] : [])}
      />
    </MemoryRouter>
  );
  const label = (pinned: boolean) => `${pinned ? "Unpin" : "Pin"} ${chat.label}`;
  const { rerender } = render(view(initialPinned, false));
  const source = screen.getByRole("button", { name: label(initialPinned) });
  source.focus();
  fireEvent.click(source);
  rerender(view(!initialPinned, true));
  expect(document.activeElement).toBe(screen.getByRole("button", { name: label(!initialPinned) }));

  await act(async () => {
    rejectPin(new Error("Request rejected"));
    await expect(pinRequest).rejects.toThrow("Request rejected");
  });
  rerender(view(initialPinned, false));

  expect(document.activeElement).toBe(screen.getByRole("button", { name: label(initialPinned) }));
});
