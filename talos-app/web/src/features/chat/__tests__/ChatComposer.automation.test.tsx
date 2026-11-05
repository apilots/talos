// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import "@/shared/i18n/setup";
import { act, cleanup, fireEvent, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { TalosApiError, AppProtocol } from "@talos/sdk-browser";
import { automationController, goal } from "@/features/automation/automation.test-support";
import { getComposerInput, renderControlledComposer, setComposerValue } from "./ChatComposer.test-utils";
vi.mock("../api", () => ({ suggestSkills: vi.fn().mockResolvedValue([]) }));
beforeEach(() => { Element.prototype.scrollIntoView = vi.fn(); });
afterEach(cleanup);
it("keeps busy-session input editable and retains the Add mode on rejected admission, consuming it only once on acceptance", async () => {
  const onSend = vi.fn().mockRejectedValueOnce(new TalosApiError({ code: AppProtocol.ErrorCode.Conflict, message: "Mode conflict", localization: null })).mockResolvedValue(undefined);
  const automation = automationController({ tasks: [{ type: "goal", value: goal() }] });
  renderControlledComposer({ initialDraft: "Check deployment", isStreaming: true, supportedAttachmentKinds: [], automation, onSend });
  expect(getComposerInput()).not.toBeDisabled();
  fireEvent.click(screen.getByRole("button", { name: "Add" }));
  fireEvent.click(screen.getByRole("button", { name: "Loop" }));
  expect(screen.getByTestId("automation-mode-chip")).toHaveTextContent("Loop");
  expect(onSend).not.toHaveBeenCalled();
  fireEvent.keyDown(getComposerInput(), { key: "Enter" });
  await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent("Mode conflict"));
  expect(getComposerInput()).toHaveValue("Check deployment");
  expect(screen.getByTestId("automation-mode-chip")).toHaveTextContent("Loop");
  fireEvent.keyDown(getComposerInput(), { key: "Enter" });
  await waitFor(() => expect(screen.queryByTestId("automation-mode-chip")).not.toBeInTheDocument());
  expect(onSend.mock.calls.slice(0, 2)).toEqual([[[], "loop"], [[], "loop"]]);
  fireEvent.keyDown(getComposerInput(), { key: "Enter" });
  await waitFor(() => expect(onSend).toHaveBeenCalledTimes(3));
  expect(onSend.mock.calls[2]).toEqual([[], undefined]);
});
it("selects slash mode while busy with no immediate task creation, just like Add", async () => {
  const onSend = vi.fn().mockResolvedValue(undefined);
  renderControlledComposer({ isStreaming: true, automation: automationController(), onSend });
  setComposerValue("/goal");
  fireEvent.keyDown(getComposerInput(), { key: "Enter" });
  expect(screen.getByTestId("automation-mode-chip")).toHaveTextContent("Goal");
  expect(getComposerInput()).toHaveValue("");
  expect(onSend).not.toHaveBeenCalled();
  setComposerValue("Fix tests");
  fireEvent.keyDown(getComposerInput(), { key: "Enter" });
  await waitFor(() => expect(onSend).toHaveBeenCalledWith([], "goal"));
});
it.each(["Goal", "Loop"])("clearing the inline %s mode retains prompt and never stops an existing task", (mode) => {
  const automation = automationController();
  renderControlledComposer({ initialDraft: "Do the work", automation });
  fireEvent.click(screen.getByRole("button", { name: "Add" }));
  fireEvent.click(screen.getByRole("button", { name: mode }));
  const clear = screen.getByRole("button", { name: "Clear selected mode" });
  expect(clear).toHaveAccessibleDescription(`Clear ${mode}`);
  fireEvent.click(clear);
  expect(screen.queryByTestId("automation-mode-chip")).not.toBeInTheDocument();
  expect(getComposerInput()).toHaveValue("Do the work");
  expect(automation.execute).not.toHaveBeenCalled();
});

it("does not submit or consume the selected mode when Enter confirms an IME composition", async () => {
  const onSend = vi.fn().mockResolvedValue(undefined);
  renderControlledComposer({ initialDraft: "修复测试", automation: automationController(), onSend });
  fireEvent.click(screen.getByRole("button", { name: "Add" }));
  fireEvent.click(screen.getByRole("button", { name: "Goal" }));
  fireEvent.keyDown(getComposerInput(), { key: "Enter", isComposing: true });
  fireEvent.compositionStart(getComposerInput());
  fireEvent.keyDown(getComposerInput(), { key: "Enter" });
  expect(onSend).not.toHaveBeenCalled();
  expect(getComposerInput()).toHaveValue("修复测试");
  expect(screen.getByTestId("automation-mode-chip")).toHaveTextContent("Goal");
  fireEvent.compositionEnd(getComposerInput());
  fireEvent.keyDown(getComposerInput(), { key: "Enter" });
  await waitFor(() => expect(onSend).toHaveBeenCalledWith([], "goal"));
  await waitFor(() => expect(screen.queryByTestId("automation-mode-chip")).not.toBeInTheDocument());
});

it("labels the shared Add menu honestly for text-only models and keeps Goal/Loop at its top level", () => {
  renderControlledComposer({ supportedAttachmentKinds: [], automation: automationController() });
  const trigger = screen.getByRole("button", { name: "Add" });
  expect(trigger).toBeEnabled();
  expect(trigger).toHaveAttribute("title", "Add");
  fireEvent.click(trigger);
  expect(screen.getByText("Add")).toBeVisible();
  expect(screen.getByRole("button", { name: "Goal" })).toBeEnabled();
  expect(screen.getByRole("button", { name: "Loop" })).toBeEnabled();
  expect(screen.getByRole("button", { name: "Upload a file" })).toBeDisabled();
  expect(screen.queryByRole("combobox")).not.toBeInTheDocument();
});


it.each(["goal", "loop"] as const)("offers %s through the same Home catalog for slash and Add without a Session controller", async (mode) => {
  const onSend = vi.fn().mockResolvedValue(undefined);
  renderControlledComposer({ initialDraft: "", supportedAttachmentKinds: [], automationCommands: [{ mode, command: `/${mode}`, description: "Shared command" }], onSend });
  expect(screen.getByRole("button", { name: "Add" })).toBeEnabled();
  setComposerValue(`/${mode}`);
  fireEvent.keyDown(getComposerInput(), { key: "Enter" });
  expect(screen.getByTestId("automation-mode-chip")).toHaveTextContent(mode === "goal" ? "Goal" : "Loop");
  expect(onSend).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "Clear selected mode" }));
  fireEvent.click(screen.getByRole("button", { name: "Add" }));
  fireEvent.click(screen.getByRole("button", { name: mode === "goal" ? "Goal" : "Loop" }));
  setComposerValue("Original task");
  fireEvent.keyDown(getComposerInput(), { key: "Enter" });
  await waitFor(() => expect(onSend).toHaveBeenCalledExactlyOnceWith([], mode));
  await waitFor(() => expect(screen.queryByTestId("automation-mode-chip")).not.toBeInTheDocument());
});

it.each(["Enter", "Send"])("retains a one-shot mode until admission and consumes it at ACK through %s", async (submit) => {
  let accept!: () => void;
  const onSend = vi.fn(() => new Promise<void>((resolve) => { accept = resolve; }));
  renderControlledComposer({ initialDraft: "Original task", automationCommands: [{ mode: "goal", command: "/goal", description: "Shared command" }], onSend });
  fireEvent.click(screen.getByRole("button", { name: "Add" }));
  fireEvent.click(screen.getByRole("button", { name: "Goal" }));
  if (submit === "Enter") fireEvent.keyDown(getComposerInput(), { key: "Enter" });
  else fireEvent.click(screen.getByRole("button", { name: "Send" }));
  await waitFor(() => expect(onSend).toHaveBeenCalledExactlyOnceWith([], "goal"));
  expect(screen.getByTestId("automation-mode-chip")).toHaveTextContent("Goal");
  fireEvent.keyDown(getComposerInput(), { key: "Enter" });
  expect(onSend).toHaveBeenCalledTimes(1);
  await act(async () => { accept(); });
  expect(screen.queryByTestId("automation-mode-chip")).not.toBeInTheDocument();
});

it("restores a rejected initial-navigation mode as a retry draft without resubmitting it", async () => {
  const onSend = vi.fn().mockResolvedValue(undefined);
  renderControlledComposer({ initialDraft: "Original task", initialAutomationMode: "loop", automationCommands: [{ mode: "loop", command: "/loop", description: "Shared command" }], onSend });
  expect(screen.getByTestId("automation-mode-chip")).toHaveTextContent("Loop");
  expect(onSend).not.toHaveBeenCalled();
  fireEvent.keyDown(getComposerInput(), { key: "Enter" });
  await waitFor(() => expect(onSend).toHaveBeenCalledExactlyOnceWith([], "loop"));
  await waitFor(() => expect(screen.queryByTestId("automation-mode-chip")).not.toBeInTheDocument());
});
