// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import "@/shared/i18n/setup";
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { TextMessageBlock } from "./TextMessageBlock";
afterEach(cleanup);
it("keeps the same user message template and adds source only to the triggering input footer", () => {
  render(<><TextMessageBlock kind="user" text="Ordinary input" /><TextMessageBlock kind="user" text="Automatic input" automationOrigin={{ type: "loop", loopId: "loop-1", occurrenceId: "o1", configRevision: 1 }} /></>);
  expect(screen.getAllByTestId("automation-input-source")).toHaveLength(1);
  expect(screen.getByTestId("automation-input-source")).toHaveTextContent("Sent as loop");
  expect(screen.getByTestId("automation-input-source")).toHaveAttribute("aria-label", "Sent as loop");
  expect(screen.queryByRole("button", { name: "Copy block" })).not.toBeInTheDocument();
});
