// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import { fireEvent, screen } from "@testing-library/react";
import { expect, it } from "vitest";
import { useAppStore } from "@/shared/store/app-store";
import { fixture, show } from "./AppLayout.test-utils";

it.each([true, false])("shows the computer homepage without sidebar or new-chat controls (wide=%s)", wide => {
  fixture.wide = wide;
  fixture.selectedId = undefined;
  useAppStore.setState({ sidebarOpen: true });
  const { container } = show("/", true);
  expect(screen.getByRole("heading", { name: "Choose a computer" })).toBeVisible();
  expect(container.querySelector("#talos-sidebar")).toBeNull();
  expect(screen.queryByRole("link", { name: "New chat" })).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: /sidebar/i })).not.toBeInTheDocument();
  expect(screen.queryByRole("group", { name: "Workspace controls" })).not.toBeInTheDocument();
  expect(document.body).toHaveFocus();
  fireEvent(window, new Event("resize"));
  expect(document.body).toHaveFocus();
});

it.each([true, false])("restores the existing work area for a selected computer (available=%s)", available => {
  fixture.selectedId = undefined;
  fixture.available = available;
  useAppStore.setState({ sidebarOpen: true });
  show("/");
  fixture.selectedId = "selected-worker";
  fireEvent(window, new Event("resize"));
  expect(screen.queryByRole("heading", { name: "Choose a computer" })).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Sidebar action" })).toBeVisible();
  expect(screen.getByRole("link", { name: "New chat" })).toBeVisible();
  expect(screen.getByText("Page content")).toBeVisible();
  expect(useAppStore.getState().sidebarOpen).toBe(true);
});

it.each(["/settings/models", "/computers", "/sessions/history"])("keeps %s accessible without a selected computer", path => {
  fixture.selectedId = undefined;
  show(path);
  expect(screen.getByText("Page content")).toBeVisible();
  expect(screen.queryByRole("heading", { name: "Choose a computer" })).not.toBeInTheDocument();
});
