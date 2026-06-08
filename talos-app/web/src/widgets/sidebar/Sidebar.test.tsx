// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { afterEach, describe, expect, it, vi } from "vitest";

import "@/shared/i18n/setup";
import { Sidebar } from "./Sidebar";

function renderSidebar(role: "user" | "admin" = "user") {
  return render(
    <MemoryRouter>
      <Sidebar
        connection={{
          authStatus: "authenticated",
          authRole: role,
          authUsername: role,
          userId: `${role}-1`,
        }}
        currentPath="/"
        isHomePage={false}
        onCreateThread={vi.fn()}
        onCycleThreadFilter={vi.fn()}
        onDeleteThread={vi.fn()}
        onLogout={vi.fn()}
        onSelectThread={vi.fn()}
        threadCount={0}
        threadFilter="all"
        threads={[]}
      />
    </MemoryRouter>,
  );
}

describe("Sidebar", () => {
  afterEach(() => {
    cleanup();
  });

  it("keeps main navigation focused on work items", () => {
    renderSidebar();

    expect(screen.getByText("Projects")).toBeTruthy();
    expect(screen.getByText("Files")).toBeTruthy();
    expect(screen.queryByText("Memory")).toBeNull();
    expect(screen.queryByText("Workspace Docs")).toBeNull();
    expect(screen.queryByText("Settings")).toBeNull();
    expect(screen.queryByText("Admin")).toBeNull();
  });

  it("shows settings from the account menu", () => {
    renderSidebar();

    fireEvent.click(screen.getByRole("button", { name: /user/i }));

    expect(screen.getByText("Settings")).toBeTruthy();
    expect(screen.getByText("Sign out")).toBeTruthy();
  });

  it("never renders Talos admin navigation", () => {
    const { container, rerender } = renderSidebar("user");
    expect(container.querySelector('a[href="/admin"]')).toBeNull();

    rerender(
      <MemoryRouter>
        <Sidebar
          connection={{
            authStatus: "authenticated",
            authRole: "admin",
            authUsername: "admin",
            userId: "admin-1",
          }}
          currentPath="/"
          isHomePage={false}
          onCreateThread={vi.fn()}
          onCycleThreadFilter={vi.fn()}
          onDeleteThread={vi.fn()}
          onLogout={vi.fn()}
          onSelectThread={vi.fn()}
          threadCount={0}
          threadFilter="all"
          threads={[]}
        />
      </MemoryRouter>,
    );

    expect(container.querySelector('a[href="/admin"]')).toBeNull();
  });
});
