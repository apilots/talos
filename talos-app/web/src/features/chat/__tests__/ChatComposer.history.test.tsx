// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";

import { cleanup, fireEvent } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import "@/shared/i18n/setup";
import {
  committedHistory,
  getComposerInput,
  moveCursorToEnd,
  renderComposer,
  renderControlledComposer,
  renderControlledComposerWithRebuiltHistory,
} from "./ChatComposer.test-utils";

describe("ChatComposer input history", () => {
  beforeEach(() => {
    vi.spyOn(window, "requestAnimationFrame").mockImplementation((callback) => {
      callback(0);
      return 0;
    });
  });

  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
  });

  it("navigates session input history from an empty composer with ArrowUp", () => {
    renderControlledComposer({ inputHistory: committedHistory("first input", "second input") });
    const textarea = getComposerInput();

    fireEvent.keyDown(textarea, { key: "ArrowUp" });
    expect(textarea).toHaveValue("second input");

    textarea.setSelectionRange(0, 0);
    fireEvent.keyDown(textarea, { key: "ArrowUp" });
    expect(textarea).toHaveValue("first input");

    fireEvent.keyDown(textarea, { key: "ArrowUp" });
    expect(textarea).toHaveValue("first input");
  });

  it("keeps history navigation when parent rerenders with the same history identity", () => {
    renderControlledComposerWithRebuiltHistory(["older input", "latest input"]);
    const textarea = getComposerInput();

    fireEvent.keyDown(textarea, { key: "ArrowUp" });
    expect(textarea).toHaveValue("latest input");

    fireEvent.keyDown(textarea, { key: "ArrowUp" });
    expect(textarea).toHaveValue("older input");
  });

  it("restores the original draft after navigating past the newest history item", () => {
    renderControlledComposer({
      initialDraft: "current draft",
      inputHistory: committedHistory("older input", "latest input"),
    });
    const textarea = getComposerInput();
    moveCursorToEnd(textarea);

    fireEvent.keyDown(textarea, { key: "ArrowUp" });
    expect(textarea).toHaveValue("latest input");

    fireEvent.keyDown(textarea, { key: "ArrowDown" });
    expect(textarea).toHaveValue("current draft");
  });

  it("does not start history navigation with ArrowDown", () => {
    renderControlledComposer({ inputHistory: committedHistory("latest input") });
    const textarea = getComposerInput();

    fireEvent.keyDown(textarea, { key: "ArrowDown" });

    expect(textarea).toHaveValue("");
  });

  it("does not add pending input to keyboard history once committed history owns its message", () => {
    renderControlledComposer({
      inputHistory: {
        committed: [
          { messageId: "msg-1", itemIndex: 0, text: "older input" },
          { messageId: "msg-2", itemIndex: 0, text: "latest input" },
        ],
        pendingInput: {
          text: "latest input",
          clientMessageId: "client-1",
          serverMessageId: "msg-2",
          phase: "completed",
        },
      },
    });
    const textarea = getComposerInput();

    fireEvent.keyDown(textarea, { key: "ArrowUp" });
    expect(textarea).toHaveValue("latest input");

    fireEvent.keyDown(textarea, { key: "ArrowUp" });
    expect(textarea).toHaveValue("older input");
  });

  it("does not add completed pending input to keyboard history before canonical history catches up", () => {
    renderControlledComposer({
      inputHistory: {
        committed: [
          { messageId: "msg-1", itemIndex: 0, text: "older input" },
          { messageId: "msg-2", itemIndex: 0, text: "latest committed input" },
        ],
        pendingInput: {
          text: "completed pending input",
          clientMessageId: "client-1",
          serverMessageId: "msg-pending",
          phase: "completed",
        },
      },
    });
    const textarea = getComposerInput();

    fireEvent.keyDown(textarea, { key: "ArrowUp" });
    expect(textarea).toHaveValue("latest committed input");

    fireEvent.keyDown(textarea, { key: "ArrowUp" });
    expect(textarea).toHaveValue("older input");
  });

  it("keeps native textarea navigation when the cursor is not at the end", () => {
    renderControlledComposer({
      initialDraft: "first line\nsecond line",
      inputHistory: committedHistory("latest input"),
    });
    const textarea = getComposerInput();
    textarea.setSelectionRange(0, 0);

    fireEvent.keyDown(textarea, { key: "ArrowUp" });

    expect(textarea).toHaveValue("first line\nsecond line");
  });

  it("resets history navigation after manual edits", () => {
    renderControlledComposer({ inputHistory: committedHistory("older input", "latest input") });
    const textarea = getComposerInput();

    fireEvent.keyDown(textarea, { key: "ArrowUp" });
    expect(textarea).toHaveValue("latest input");

    fireEvent.change(textarea, { target: { value: "manual edit" } });
    moveCursorToEnd(textarea);
    fireEvent.keyDown(textarea, { key: "ArrowDown" });

    expect(textarea).toHaveValue("manual edit");
  });

  it("does not navigate history during IME composition or modifier key shortcuts", () => {
    renderControlledComposer({ inputHistory: committedHistory("latest input") });
    const textarea = getComposerInput();
    const composingEvent = new KeyboardEvent("keydown", { bubbles: true, key: "ArrowUp" });
    Object.defineProperty(composingEvent, "isComposing", { value: true });

    fireEvent(textarea, composingEvent);
    expect(textarea).toHaveValue("");

    fireEvent.keyDown(textarea, { ctrlKey: true, key: "ArrowUp" });
    expect(textarea).toHaveValue("");
  });

  it("keeps Enter send behavior separate from history navigation and streaming state", () => {
    const onSend = vi.fn();
    renderControlledComposer({
      initialDraft: "send this",
      inputHistory: committedHistory("latest input"),
      onSend,
    });
    const textarea = getComposerInput();

    fireEvent.keyDown(textarea, { key: "Enter" });

    expect(onSend).toHaveBeenCalledTimes(1);

    cleanup();
    renderComposer({ draft: "send this later", isStreaming: true, onSend });
    fireEvent.keyDown(getComposerInput(), { key: "Enter" });
    expect(onSend).toHaveBeenCalledTimes(1);
  });
});
