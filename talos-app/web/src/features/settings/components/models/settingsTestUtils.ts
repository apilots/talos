import userEvent from "@testing-library/user-event";
import { screen } from "@testing-library/react";
import { afterEach, beforeEach, vi } from "vitest";

export function installRadixTestEnvironment() {
  const names = ["hasPointerCapture", "setPointerCapture", "releasePointerCapture", "scrollIntoView"] as const;
  const originals = names.map((name) => [name, Object.getOwnPropertyDescriptor(HTMLElement.prototype, name)] as const);
  beforeEach(() => {
    vi.stubGlobal("ResizeObserver", class { observe() {} unobserve() {} disconnect() {} });
    for (const name of names) Object.defineProperty(HTMLElement.prototype, name, { configurable: true, value: name === "hasPointerCapture" ? () => false : () => {} });
  });
  afterEach(() => {
    vi.unstubAllGlobals();
    for (const [name, original] of originals) {
      if (original) Object.defineProperty(HTMLElement.prototype, name, original);
      else Reflect.deleteProperty(HTMLElement.prototype, name);
    }
  });
}

export async function selectOption(trigger: HTMLElement, name: string) {
  const user = userEvent.setup();
  await user.click(trigger);
  await user.click(await screen.findByRole("option", { name }));
}

export async function chooseExample(label: string, example: string) {
  const user = userEvent.setup();
  await user.click(screen.getByRole("button", { name: label }));
  await user.click(await screen.findByRole("menuitem", { name: example }));
}
