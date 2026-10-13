// @vitest-environment jsdom

import { cleanup, fireEvent, render, renderHook, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";

import "@/shared/i18n/setup";
import { useSettingsPage } from "../hooks/useSettingsPage";
import { SettingsServerConnectionPanel } from "./SettingsServerConnectionPanel";

const refetch = vi.fn();
vi.mock("@/features/chat", () => ({
  useModelsQuery: () => ({ refetch }),
}));

afterEach(cleanup);

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
