// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import "@/shared/i18n/setup";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { AutomationEditDialog } from "./AutomationEditDialog";
import { loop, goal } from "./automation.test-support";
afterEach(cleanup);
it("edits only Goal prompt at its exact ID/revision and retains the original budget", async () => {
  const save = vi.fn().mockResolvedValue(undefined); const close = vi.fn();
  render(<AutomationEditDialog task={{ type: "goal", value: goal({ id: "historical-id" }) }} pending={false} onSave={save} onClose={close} />);
  expect(screen.queryByRole("combobox")).not.toBeInTheDocument();
  expect(screen.getByText(/budget.*unchanged/)).toBeInTheDocument();
  fireEvent.change(screen.getByRole("textbox"), { target: { value: "Finish migration" } });
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  await waitFor(() => expect(close).toHaveBeenCalledOnce());
  expect(save).toHaveBeenCalledWith({ target: "goal", action: { type: "edit", goalId: "historical-id", expectedRevision: 2, objective: "Finish migration" } });
});
it("validates fixed minutes then edits the same Loop and never creates or restarts it", async () => {
  const save = vi.fn().mockResolvedValue(undefined);
  render(<AutomationEditDialog task={{ type: "loop", value: loop() }} pending={false} onSave={save} onClose={vi.fn()} />);
  fireEvent.change(screen.getByRole("combobox"), { target: { value: "fixedInterval" } });
  fireEvent.change(screen.getByRole("spinbutton"), { target: { value: "0" } });
  expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
  fireEvent.change(screen.getByRole("spinbutton"), { target: { value: "2" } });
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  await waitFor(() => expect(save).toHaveBeenCalledWith({ target: "loop", action: { type: "edit", loopId: "loop-1", expectedRevision: 3, prompt: "Check CI", schedule: { type: "fixedInterval", minutes: 2 } } }));
});
it("keeps an unsuccessful edit open", async () => {
  const close = vi.fn();
  render(<AutomationEditDialog task={{ type: "loop", value: loop() }} pending={false} error="Concurrent edit" onSave={vi.fn().mockRejectedValue(new Error("conflict"))} onClose={close} />);
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent("Concurrent edit"));
  expect(close).not.toHaveBeenCalled();
});
