// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import "@/shared/i18n/setup";
import { useRef, useState } from "react";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { AutomationEditDialog } from "./AutomationEditDialog";
import { goal } from "./automation.test-support";
afterEach(cleanup);
function Harness({ removeTrigger = false }: { removeTrigger?: boolean }) {
  const [open, setOpen] = useState(false);
  const fallback = useRef<HTMLTextAreaElement>(null);
  return <><textarea ref={fallback} aria-label="Composer" />
    {!removeTrigger ? <button onClick={(event) => { event.currentTarget.focus(); setOpen(true); }}>Edit Goal</button> : null}
    {open ? <AutomationEditDialog task={{ type: "goal", value: goal() }} pending={false} fallbackFocusRef={fallback} onSave={vi.fn().mockResolvedValue(undefined)} onClose={() => setOpen(false)} /> : null}
  </>;
}
it.each(["escape", "save"])("restores the explicit editing trigger after %s", async (close) => {
  render(<Harness />);
  const trigger = screen.getByRole("button", { name: "Edit Goal" });
  expect(document.activeElement).toBe(document.body);
  fireEvent.click(trigger);
  await waitFor(() => expect(screen.getByRole("textbox", { name: "Instructions" })).toHaveFocus());
  if (close === "escape") fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });
  else fireEvent.click(screen.getByRole("button", { name: "Save" }));
  await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
  await waitFor(() => expect(trigger).toHaveFocus());
});
it("returns to the current composer if the editing action disappeared", async () => {
  const view = render(<Harness />);
  fireEvent.click(screen.getByRole("button", { name: "Edit Goal" }));
  await screen.findByRole("dialog");
  view.rerender(<Harness removeTrigger />);
  fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });
  await waitFor(() => expect(screen.getByRole("textbox", { name: "Composer" })).toHaveFocus());
});
