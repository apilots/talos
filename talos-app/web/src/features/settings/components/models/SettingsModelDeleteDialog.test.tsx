// @vitest-environment jsdom
import userEvent from "@testing-library/user-event";
import "@testing-library/jest-dom/vitest";
import { installRadixTestEnvironment } from "./settingsTestUtils";

import { StrictMode } from "react";
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";

import "@/shared/i18n/setup";
import { SettingsModelDeleteDialog } from "./SettingsModelDeleteDialog";

installRadixTestEnvironment();

const externalButtons: HTMLButtonElement[] = [];
afterEach(() => {
  cleanup();
  externalButtons.splice(0).forEach((button) => button.remove());
});

it("traps keyboard focus and preserves focus moved elsewhere after Strict Mode unmount", async () => {
  const previous = document.createElement("button");
  const elsewhere = document.createElement("button");
  externalButtons.push(previous, elsewhere);
  document.body.append(previous, elsewhere);
  previous.focus();
  const fallbackFocusRef = { current: previous };
  const { unmount } = render(
    <StrictMode>
      <SettingsModelDeleteDialog fallbackFocusRef={fallbackFocusRef} kind="provider" name="LLM Gateway" onClose={vi.fn()} onConfirm={vi.fn()} />
    </StrictMode>,
  );
  expect(screen.getByRole("button", { name: "Cancel" })).toHaveFocus();
  screen.getByRole("button", { name: "Close" }).focus();
  await userEvent.tab({ shift: true });
  expect(screen.getByRole("button", { name: "Delete" })).toHaveFocus();
  await userEvent.tab();
  expect(screen.getByRole("button", { name: "Close" })).toHaveFocus();
  unmount();
  elsewhere.focus();
  await waitFor(() => expect(elsewhere).toHaveFocus());
});
