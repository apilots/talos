// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import "@/shared/i18n/setup";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { ResourceOpenMenu } from "./ResourceOpenMenu";
import { useAppStore } from "@/shared/store/app-store";
import { workspaceWindowKey } from "@/shared/store/workspace-window";
const open = vi.hoisted(() => vi.fn());
vi.mock("@/shared/lib/open-resource", async importOriginal => ({ ...await importOriginal<object>(), openResource: open }));
beforeEach(() => {
  open.mockReset().mockResolvedValue(undefined);
  useAppStore.setState({ workspaceOwner: null, workspaceSnapshots: {}, workspaceWindows: [], workspaceActiveKey: null, workspaceOpen: false });
  useAppStore.getState().setWorkspaceContext({ scope: "actor", context: { kind: "session", sessionId: "a" } }, true);
  useAppStore.getState().openWorkspaceWindow({ kind: "file", fileId: "report" });
});
afterEach(cleanup);
it("copies a stable file reference and closes the keyboard menu to its trigger", async () => {
  const user = userEvent.setup();
  const copy = vi.spyOn(navigator.clipboard, "writeText").mockResolvedValue();
  render(<ResourceOpenMenu resource={{ kind: "file", fileId: "report" }} />);
  const trigger = screen.getByRole("button", { name: "Open in options" });
  trigger.focus(); await user.keyboard("{Enter}");
  expect(screen.getByRole("menuitem", { name: "Talos" })).toBeVisible();
  await user.keyboard("{End}{Enter}");
  expect(copy).toHaveBeenCalledWith("talos-file://report");
  expect(trigger).toHaveFocus();
});
it("routes explicit Talos and external actions without changing a global preference", async () => {
  const user = userEvent.setup();
  render(<ResourceOpenMenu resource={{ kind: "webpage", url: "https://example.test/" }} />);
  await user.click(screen.getByRole("button", { name: "Open in" }));
  expect(open).toHaveBeenLastCalledWith({ kind: "webpage", url: "https://example.test/", intent: "preview" });
  await user.click(screen.getByRole("button", { name: "Open in options" }));
  await user.click(screen.getByRole("menuitem", { name: "Open in external browser" }));
  expect(open).toHaveBeenLastCalledWith({ kind: "webpage", url: "https://example.test/", intent: "external" });
});

function RetainedMenu() {
  const owner = useAppStore(state => state.workspaceOwner);
  const active = useAppStore(state => state.workspaceActiveKey);
  const visible = useAppStore(state => state.workspaceOpen);
  const selected = owner?.context.kind === "session" && owner.context.sessionId === "a"
    && active === workspaceWindowKey({ kind: "file", fileId: "report" }) && visible;
  return <div hidden={!selected} inert={!selected}><ResourceOpenMenu resource={{ kind: "file", fileId: "report" }} /></div>;
}
it.each(["context", "selection", "collapse"])("retires a retained menu when its %s changes without focusing the hidden trigger", async change => {
  const user = userEvent.setup();
  render(<RetainedMenu />);
  const trigger = screen.getByRole("button", { name: "Open in options" });
  await user.click(trigger);
  expect(screen.getByRole("menuitem", { name: "Talos" })).toBeVisible();
  act(() => {
    const store = useAppStore.getState();
    if (change === "context") store.setWorkspaceContext({ scope: "actor", context: { kind: "session", sessionId: "b" } }, true);
    else if (change === "selection") store.openWorkspaceWindow({ kind: "file", fileId: "other" });
    else store.setWorkspaceOpen(false);
  });
  await waitFor(() => expect(screen.queryByRole("menuitem")).not.toBeInTheDocument());
  expect(trigger).not.toHaveFocus();
});

it("closes the opening menu when its surrounding document scrolls", async () => {
  const user = userEvent.setup();
  render(<ResourceOpenMenu resource={{ kind: "file", fileId: "report" }} />);
  await user.click(screen.getByRole("button", { name: "Open in options" }));
  expect(screen.getByRole("menuitem", { name: "Talos" })).toBeVisible();
  fireEvent.scroll(document);
  await waitFor(() => expect(screen.queryByRole("menu")).not.toBeInTheDocument());
});
