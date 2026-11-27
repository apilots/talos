// @vitest-environment jsdom

import { act, cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import { useAppIconEffects } from "@/shared/app-icons/useAppIconEffects";
import "@/shared/i18n/setup";
import { useAppStore } from "@/shared/store/app-store";

import { SettingsAppIcons } from "./SettingsAppIcons";

function Appearance() {
  useAppIconEffects();
  return <SettingsAppIcons />;
}

beforeEach(() => {
  useAppStore.getState().setAppIconId("portrait-light");
  window.localStorage.clear();
});

afterEach(() => {
  cleanup();
  document.head.querySelector("[data-app-icon-touch]")?.remove();
  window.localStorage.clear();
  vi.unstubAllGlobals();
});

it("updates the Apple Touch icon when the selection changes", async () => {
  const user = userEvent.setup();
  document.head.insertAdjacentHTML("beforeend", '<link data-app-icon-touch rel="apple-touch-icon">');
  render(<Appearance />);
  await user.click(screen.getByRole("radio", { name: "Portrait dark" }));
  expect(document.head.querySelector("[data-app-icon-touch]")?.getAttribute("href"))
    .toBe("/app-icons/portrait/apple-touch-icon.png");
});

it.each([{}, { appIconId: "unknown" }])("restores the default icon for stored preferences %j", async (state) => {
  useAppStore.getState().setAppIconId("classic");
  window.localStorage.setItem("talos-web-ui", JSON.stringify({ state, version: 0 }));
  await useAppStore.persist.rehydrate();
  render(<Appearance />);

  expect((screen.getByRole<HTMLInputElement>("radio", { name: "Portrait light" })).checked).toBe(true);
});

it("passes icon IDs to Desktop and reports only failures for the active choice", async () => {
  const user = userEvent.setup();
  let rejectFirstClassic = () => {};
  const setAppIcon = vi.fn()
    .mockResolvedValueOnce(undefined)
    .mockImplementationOnce(() => new Promise<void>((_, reject) => {
      rejectFirstClassic = () => reject(new Error("Stale native error"));
    }))
    .mockResolvedValueOnce(undefined)
    .mockRejectedValueOnce(new Error("Current native error"));
  vi.stubGlobal("talos", { setAppIcon });

  render(<Appearance />);
  await user.click(screen.getByRole<HTMLInputElement>("radio", { name: "Talos classic" }));
  await user.click(screen.getByRole("radio", { name: "Portrait dark" }));
  await act(() => Promise.resolve(rejectFirstClassic()));
  expect(screen.queryByRole("alert")).toBeNull();
  expect(setAppIcon.mock.calls).toEqual([["portrait-light"], ["classic"], ["portrait"]]);

  await user.click(screen.getByRole<HTMLInputElement>("radio", { name: "Talos classic" }));
  await waitFor(() => expect(screen.getByRole("alert").textContent).toBe("Could not update the system app icon."));
  expect((screen.getByRole<HTMLInputElement>("radio", { name: "Talos classic" })).checked).toBe(true);
});
