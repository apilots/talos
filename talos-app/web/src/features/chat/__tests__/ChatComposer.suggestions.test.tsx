// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";

import { cleanup, fireEvent, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import "@/shared/i18n/setup";
import {
  getComposerInput,
  getComposerValue,
  renderControlledComposer,
  setComposerSelection,
  setComposerValue,
} from "./ChatComposer.test-utils";

const apiMocks = vi.hoisted(() => ({ suggestSkills: vi.fn() }));
const scrollIntoViewMock = vi.fn();

vi.mock("../api", () => ({ suggestSkills: apiMocks.suggestSkills }));

const SKILLS = [
  {
    name: "algorithmic-art",
    description: "Creating algorithmic art using p5.js with seeded randomness.",
    short_description: null,
    root: "/Users/test/.agents/skills/algorithmic-art",
    path: "/Users/test/.agents/skills/algorithmic-art/SKILL.md",
    scope: "external" as const,
    source_plugin: null,
    source_install: null,
    metadata: null,
  },
  {
    name: "browser",
    description: "Control the in-app browser with Codex.",
    short_description: null,
    root: "/Users/test/.talos/skills/browser",
    path: "/Users/test/.talos/skills/browser/SKILL.md",
    scope: "plugin" as const,
    source_plugin: "browser",
    source_install: "browser",
    metadata: null,
  },
];

describe("ChatComposer suggestions", () => {
  beforeEach(() => {
    apiMocks.suggestSkills.mockResolvedValue(SKILLS);
    Object.defineProperty(HTMLElement.prototype, "scrollIntoView", {
      configurable: true,
      value: scrollIntoViewMock,
    });
    vi.spyOn(window, "requestAnimationFrame").mockImplementation((callback) =>
      window.setTimeout(() => callback(0), 0),
    );
  });

  afterEach(() => {
    cleanup();
    apiMocks.suggestSkills.mockReset();
    scrollIntoViewMock.mockReset();
    Reflect.deleteProperty(HTMLElement.prototype, "scrollIntoView");
    vi.restoreAllMocks();
  });

  it("filters and executes available slash commands", () => {
    const onCommand = vi.fn();
    renderControlledComposer({ initialDraft: "/c", onCommand });

    expect(screen.getByText("Compact")).toBeInTheDocument();
    expect(screen.getByText("Clear")).toBeInTheDocument();
    expect(screen.queryByText("Status")).not.toBeInTheDocument();

    fireEvent.keyDown(getComposerInput(), { key: "ArrowDown" });
    fireEvent.keyDown(getComposerInput(), { key: "Enter" });

    expect(onCommand).toHaveBeenCalledWith("clear");
    expect(getComposerValue()).toBe("");
  });

  it("refreshes Skills when a new slash session starts at the same position", async () => {
    const onCommand = vi.fn();
    renderControlledComposer({ initialDraft: "/", onCommand });
    await waitFor(() => expect(apiMocks.suggestSkills).toHaveBeenCalledTimes(1));

    fireEvent.mouseDown(screen.getByText("Status"));
    expect(onCommand).toHaveBeenCalledWith("status");
    expect(getComposerValue()).toBe("");
    await waitFor(() =>
      expect(screen.queryByTestId("composer-suggestions")).not.toBeInTheDocument(),
    );

    setComposerValue("/");

    expect(screen.getByTestId("composer-suggestions")).toBeInTheDocument();
    expect(screen.getByText("Status")).toBeInTheDocument();
    await waitFor(() => expect(apiMocks.suggestSkills).toHaveBeenCalledTimes(2));
  });

  it("retries a failed Skill request in the next slash session", async () => {
    apiMocks.suggestSkills
      .mockRejectedValueOnce(new Error("temporary failure"))
      .mockResolvedValueOnce(SKILLS);
    renderControlledComposer({ initialDraft: "/", onCommand: vi.fn() });
    expect(await screen.findByText("temporary failure")).toBeInTheDocument();

    setComposerValue("");
    setComposerValue("/");

    expect(await screen.findByText("Browser")).toBeInTheDocument();
    expect(apiMocks.suggestSkills).toHaveBeenCalledTimes(2);
  });

  it("keeps only inline Skill suggestions available while a run is busy", async () => {
    renderControlledComposer({ initialDraft: "/", isStreaming: true, onCommand: vi.fn() });

    await screen.findByText("Browser");

    expect(
      within(screen.getByTestId("composer-suggestions")).getAllByRole("option"),
    ).toHaveLength(SKILLS.length);
  });

  it("shows measured context usage beside the compact command", () => {
    renderControlledComposer({
      contextUsage: {
        usedTokens: 17,
        contextWindowTokens: 1_000,
        effectivePromptBudgetTokens: 900,
        measuredAt: 1,
      },
      initialDraft: "/co",
      onCommand: vi.fn(),
    });

    expect(screen.getByText("Compact the current session context (1.7% used)")).toBeInTheDocument();
  });

  it("opens the model picker without dispatching a session command", () => {
    const onCommand = vi.fn();
    renderControlledComposer({ initialDraft: "/m", onCommand });

    fireEvent.keyDown(getComposerInput(), { key: "Enter" });

    expect(onCommand).not.toHaveBeenCalled();
    expect(screen.getByText("Provider")).toBeInTheDocument();
    expect(getComposerValue()).toBe("");
  });

  it("consumes an asynchronous command and preserves the current draft", async () => {
    const onCommand = vi.fn().mockRejectedValue(new Error("session is busy"));
    renderControlledComposer({ initialDraft: "/clear", onCommand });

    fireEvent.keyDown(getComposerInput(), { key: "Enter" });

    expect(getComposerValue()).toBe("");
    setComposerValue("keep this draft");

    await waitFor(() => expect(onCommand).toHaveBeenCalledWith("clear"));
    expect(getComposerValue()).toBe("keep this draft");
  });

  it("dismisses the list with Escape", () => {
    renderControlledComposer({ initialDraft: "/", onCommand: vi.fn() });

    expect(screen.getByTestId("composer-suggestions")).toBeInTheDocument();
    fireEvent.keyDown(getComposerInput(), { key: "Escape" });

    expect(screen.queryByTestId("composer-suggestions")).not.toBeInTheDocument();
  });

  it("reopens an escaped trigger only after leaving its input session", () => {
    renderControlledComposer({ initialDraft: "/", onCommand: vi.fn() });

    fireEvent.keyDown(getComposerInput(), { key: "Escape" });
    expect(screen.queryByTestId("composer-suggestions")).not.toBeInTheDocument();

    fireEvent.keyUp(getComposerInput());
    expect(screen.queryByTestId("composer-suggestions")).not.toBeInTheDocument();

    setComposerValue("");
    setComposerValue("/");
    expect(screen.getByTestId("composer-suggestions")).toBeInTheDocument();
  });

  it("reserves the final list height while the leading slash query loads", async () => {
    apiMocks.suggestSkills.mockResolvedValueOnce([]);
    renderControlledComposer({ initialDraft: "/" });

    const menu = screen.getByTestId("composer-suggestions");
    expect(menu).toHaveClass("min-h-[min(420px,42vh)]");

    await waitFor(() => expect(apiMocks.suggestSkills).toHaveBeenCalledWith(""));
    await waitFor(() => expect(menu).not.toHaveClass("min-h-[min(420px,42vh)]"));
    expect(screen.getByText("Model")).toBeInTheDocument();
  });

  it("scrolls the keyboard-selected option into the popup viewport", async () => {
    apiMocks.suggestSkills.mockResolvedValueOnce(
      Array.from({ length: 12 }, (_, index) => ({
        ...SKILLS[0],
        name: `skill-${index}`,
        path: `/Users/test/.agents/skills/skill-${index}/SKILL.md`,
      })),
    );
    renderControlledComposer({ initialDraft: "$" });
    await screen.findByText("Skill 0");
    scrollIntoViewMock.mockClear();

    for (let index = 0; index < 8; index += 1) {
      fireEvent.keyDown(getComposerInput(), { key: "ArrowDown" });
    }

    const selectedOption = screen.getByRole("option", { selected: true });
    expect(selectedOption).toHaveTextContent("Skill 8");
    expect(scrollIntoViewMock).toHaveBeenLastCalledWith({
      block: "nearest",
      inline: "nearest",
    });
    expect(scrollIntoViewMock.mock.instances.at(-1)).toBe(selectedOption);
  });

  it("follows actual pointer movement when choosing a suggestion", () => {
    renderControlledComposer({ initialDraft: "/", onCommand: vi.fn() });

    fireEvent.mouseMove(screen.getByRole("option", { name: /status/i }));

    expect(screen.getByRole("option", { selected: true })).toHaveTextContent("Status");
  });

  it("renders the reference-aligned Skill rows and inserts a plain mention", async () => {
    renderControlledComposer({ initialDraft: "$" });

    expect(await screen.findByText("Algorithmic Art")).toBeInTheDocument();
    expect(screen.getByText("Creating algorithmic art using p5.js with seeded randomness.")).toBeInTheDocument();
    expect(screen.getByText("External")).toBeInTheDocument();
    expect(screen.getByText("Plugin")).toBeInTheDocument();

    fireEvent.mouseDown(screen.getByText("Browser"));

    const input = getComposerInput();
    expect(input.value).toBe("$browser ");
    await waitFor(() => expect(input.selectionStart).toBe(input.value.length));
    expect(screen.queryByTestId("composer-suggestions")).not.toBeInTheDocument();
  });

  it("starts a new inline suggestion session after inserting a Skill", async () => {
    renderControlledComposer({ initialDraft: "$" });
    fireEvent.mouseDown(await screen.findByText("Browser"));
    await waitFor(() => expect(getComposerInput().selectionStart).toBe("$browser ".length));

    setComposerValue("$browser $");

    expect(await screen.findByText("Algorithmic Art")).toBeInTheDocument();
    expect(screen.getByTestId("composer-suggestions")).toBeInTheDocument();
  });

  it("replaces a whitespace-delimited body slash at the active cursor", async () => {
    renderControlledComposer({ initialDraft: "please /bro now" });
    const input = getComposerInput();
    setComposerSelection(11, input);

    await waitFor(() => expect(apiMocks.suggestSkills).toHaveBeenLastCalledWith("bro"));
    fireEvent.mouseDown(screen.getByText("Browser"));

    expect(input.value).toBe("please $browser now");
    await waitFor(() => expect(input.selectionStart).toBe(15));
  });

  it("does not open a body slash query without leading whitespace", () => {
    renderControlledComposer({ initialDraft: "please:/bro" });

    expect(screen.queryByTestId("composer-suggestions")).not.toBeInTheDocument();
    expect(apiMocks.suggestSkills).not.toHaveBeenCalled();
  });

});
