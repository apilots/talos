// @vitest-environment jsdom

import { cleanup, fireEvent, render, renderHook, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";

import "@/shared/i18n/setup";
import { useSettingsPage } from "../hooks/useSettingsPage";
import { SettingsServerConnectionPanel } from "./SettingsServerConnectionPanel";

const refetch = vi.fn();
const desktop = vi.hoisted(() => ({ active: false }));
vi.mock("@/features/chat", () => ({
  useModelsQuery: () => ({ refetch }),
}));
vi.mock("@/shared/api/desktop-client", () => ({
  isDesktopRuntime: () => desktop.active,
}));

afterEach(() => {
  desktop.active = false;
  cleanup();
});

it("keeps login-managed credentials read-only and does not save an unchanged connection", () => {
  const { result } = renderHook(() => useSettingsPage());
  const setConnection = vi.fn();
  render(<SettingsServerConnectionPanel {...result.current} setConnection={setConnection} />);

  expect(screen.getByTitle("Managed by login, not editable here")).toHaveProperty("readOnly", true);
  const save = screen.getByRole("button", { name: "Save" });
  expect(save).toHaveProperty("disabled", true);
  fireEvent.click(save);
  fireEvent.submit(save.closest("form")!);
  expect(setConnection).not.toHaveBeenCalled();
  expect(refetch).not.toHaveBeenCalled();
});

it("shows the live Desktop connection and puts the account ID behind diagnostics", async () => {
  desktop.active = true;
  const { result } = renderHook(() => useSettingsPage());
  const connection = {
    ...result.current.connection,
    authStatus: "authenticated" as const,
    authUsername: "Example account",
    userId: "user_internal",
    workerRuntimeState: { kind: "restarting" as const, message: null },
  };
  const descriptor = Object.getOwnPropertyDescriptor(navigator, "clipboard");
  const writeText = vi.fn().mockResolvedValue(undefined);
  Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText } });
  try {
    const { rerender } = render(<SettingsServerConnectionPanel {...result.current} connection={connection} />);
    expect(screen.getByText("Example account")).toBeTruthy();
    expect(screen.getByText("Reconnecting")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Save" })).toBeNull();
    expect(screen.queryByText("Auth token")).toBeNull();
    const details = screen.getByText("Diagnostics").closest("details")!;
    expect(details.open).toBe(false);
    details.open = true;
    fireEvent.click(screen.getByRole("button", { name: "Copy account ID" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "Copied" })).toBeTruthy());
    expect(writeText).toHaveBeenCalledWith("user_internal");
    rerender(<SettingsServerConnectionPanel {...result.current} connection={{
      ...connection, workerRuntimeState: { kind: "ready", message: null },
    }} />);
    expect(screen.getByText("Connected")).toBeTruthy();
  } finally {
    if (descriptor) Object.defineProperty(navigator, "clipboard", descriptor);
    else Reflect.deleteProperty(navigator, "clipboard");
  }
});
