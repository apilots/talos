// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import "@/shared/i18n/setup";
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { MessageList } from "./MessageList";
import { TextMessageBlock } from "./TextMessageBlock";

const clipboardDescriptor = Object.getOwnPropertyDescriptor(navigator, "clipboard");
afterEach(() => {
  cleanup(); vi.useRealTimers(); vi.restoreAllMocks();
  if (clipboardDescriptor) Object.defineProperty(navigator, "clipboard", clipboardDescriptor);
  else Reflect.deleteProperty(navigator, "clipboard");
});
const createdAt = 1_700_000_000;
it.each(["goal", "loop"] as const)("keeps %s provenance visible while revealing time and copy only on hover or focus", (mode) => {
  const origin = mode === "goal" ? { type: "goal" as const, goalId: "g1", objectiveRevision: 1 } : { type: "loop" as const, loopId: "l1", occurrenceId: "o1", configRevision: 1 };
  const view = render(<><MessageList blocks={[{ id: "trigger", kind: "user", text: "same input", fileIds: [], createdAt, automationOrigin: origin }, { id: "human-steer", kind: "user", text: "same input", fileIds: [], createdAt }]} emptyTitle="" emptyDetail="" /><button>Outside</button></>);
  const root = view.container.querySelector('[data-chat-user-block-id="trigger"]')!;
  const input = within(root as HTMLElement).getByRole("group", { name: "User input" });
  const body = within(input).getByTestId("user-input-body");
  const footer = within(input).getByTestId("user-input-metadata");
  const source = screen.getByTestId("automation-input-source");
  expect(body.contains(footer)).toBe(false);
  expect(body.compareDocumentPosition(footer) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  expect(source).toHaveTextContent(`Sent as ${mode}`);
  expect(source.querySelector("svg")).toHaveAttribute("width", "14");
  expect(source.querySelector("svg")).toHaveAttribute("stroke-width", "1.3");
  expect(screen.queryByRole("time")).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Copy block" })).not.toBeInTheDocument();
  fireEvent.pointerEnter(input);
  expect(source).toHaveTextContent(`Sent as ${mode}`);
  expect(within(input).getByRole("time")).toHaveAttribute("datetime", new Date(createdAt * 1000).toISOString());
  expect(within(input).getByRole("button", { name: "Copy block" })).toBeInTheDocument();
  fireEvent.pointerLeave(input);
  expect(source).toHaveTextContent(`Sent as ${mode}`);
  expect(screen.queryByRole("time")).not.toBeInTheDocument();
  act(() => input.focus());
  expect(source).toHaveTextContent(`Sent as ${mode}`);
  act(() => screen.getByRole("button", { name: "Outside" }).focus());
  expect(source).toHaveTextContent(`Sent as ${mode}`);
  expect(screen.queryByRole("button", { name: "Copy block" })).not.toBeInTheDocument();
  expect(screen.getAllByTestId("automation-input-source")).toHaveLength(1);
});
it("does not fabricate an unknown timestamp or add a source to ordinary user input", () => {
  render(<TextMessageBlock kind="user" text={"Long text ".repeat(1000)} messageId="ordinary" />);
  const input = screen.getByRole("group", { name: "User input" });
  fireEvent.pointerEnter(input);
  expect(screen.queryByRole("time")).not.toBeInTheDocument();
  expect(screen.queryByTestId("automation-input-source")).not.toBeInTheDocument();
  expect(screen.getByTestId("user-input-body")).toHaveTextContent("Long text ".repeat(1000).trim());
  expect(screen.getByTestId("user-input-body").className).not.toMatch(/max-h-|line-clamp-|overflow-hidden/);
});
it("reveals the same metadata by touching the Input and keeps keyboard focus within it", () => {
  render(<TextMessageBlock kind="user" text="Touch input" createdAt={createdAt} automationOrigin={{ type: "loop", loopId: "l1", occurrenceId: "o1", configRevision: 1 }} />);
  const input = screen.getByRole("group", { name: "User input" });
  const touch = new Event("pointerdown", { bubbles: true });
  Object.defineProperty(touch, "pointerType", { value: "touch" });
  fireEvent(input, touch);
  expect(input).toHaveFocus();
  expect(screen.getByTestId("automation-input-source")).toHaveTextContent("Sent as loop");
  act(() => screen.getByRole("button", { name: "Copy block" }).focus());
  expect(screen.getByRole("time")).toBeInTheDocument();
});
it("copies only the original Input text using the existing clipboard feedback", async () => {
  const writeText = vi.fn().mockResolvedValue(undefined);
  Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText } });
  const text = "  Original **input**\nwith a second line";
  render(<TextMessageBlock kind="user" text={text} createdAt={createdAt} />);
  act(() => screen.getByRole("group", { name: "User input" }).focus());
  fireEvent.click(screen.getByRole("button", { name: "Copy block" }));
  await waitFor(() => expect(writeText).toHaveBeenCalledWith(text));
  expect(await screen.findByRole("button", { name: "Copied block" })).toBeInTheDocument();
});

it("resets the original copied feedback after 1200ms", async () => {
  vi.useFakeTimers();
  Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText: vi.fn().mockResolvedValue(undefined) } });
  render(<TextMessageBlock kind="user" text="Copy feedback" />);
  act(() => screen.getByRole("group", { name: "User input" }).focus());
  await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Copy block" })); });
  expect(screen.getByRole("button", { name: "Copied block" })).toBeInTheDocument();
  act(() => vi.advanceTimersByTime(1200));
  expect(screen.getByRole("button", { name: "Copy block" })).toBeInTheDocument();
});
it("keeps the Input copy control focused when the original clipboard fallback is used", async () => {
  Object.defineProperty(navigator, "clipboard", { configurable: true, value: undefined });
  const original = Object.getOwnPropertyDescriptor(document, "execCommand");
  const execCommand = vi.fn(() => { document.querySelector("textarea")?.focus(); return true; });
  Object.defineProperty(document, "execCommand", { configurable: true, value: execCommand });
  try {
    render(<TextMessageBlock kind="user" text="Fallback text" />);
    act(() => screen.getByRole("group", { name: "User input" }).focus());
    const copy = screen.getByRole("button", { name: "Copy block" });
    act(() => copy.focus());
    await act(async () => { fireEvent.click(copy); });
    expect(execCommand).toHaveBeenCalledWith("copy");
    expect(screen.getByRole("button", { name: "Copied block" })).toHaveFocus();
    expect(document.querySelector("textarea")).toBeNull();
  } finally {
    if (original) Object.defineProperty(document, "execCommand", original);
    else Reflect.deleteProperty(document, "execCommand");
  }
});


it.each(["goal", "loop"] as const)("shows an explicitly selected %s source before any automation binding exists", (mode) => {
  render(<MessageList blocks={[{ id: "selected", kind: "user", text: "same text", fileIds: [], activationMode: mode }, { id: "ordinary-steer", kind: "user", text: "same text", fileIds: [] }]} emptyTitle="" emptyDetail="" />);
  expect(screen.getAllByTestId("automation-input-source")).toHaveLength(1);
  expect(screen.getByTestId("automation-input-source")).toHaveTextContent(`Sent as ${mode}`);
  expect(screen.queryByRole("time")).not.toBeInTheDocument();
});

it("prefers the actual bound origin without replacing its identity with a selected draft mode", () => {
  render(<TextMessageBlock kind="user" text="original input" activationMode="loop" automationOrigin={{ type: "goal", goalId: "real-goal", objectiveRevision: 1 }} />);
  expect(screen.getByTestId("automation-input-source")).toHaveTextContent("Sent as goal");
});
