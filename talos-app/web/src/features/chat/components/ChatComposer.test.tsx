// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";

import { useState } from "react";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import "@/shared/i18n/setup";
import { createDefaultProjects, UNBOUND_PROJECT_ID } from "@/shared/types/domain";
import type { SessionInputHistory } from "@/shared/types/ui";
import { ChatComposer } from "./ChatComposer";

function renderComposer(overrides: Partial<React.ComponentProps<typeof ChatComposer>> = {}) {
  return render(
    <ChatComposer
      activeProjectId={UNBOUND_PROJECT_ID}
      draft=""
      isStreaming={false}
      models={[{ id: "model-1", name: "Model 1" }]}
      onCancel={vi.fn()}
      onDraftChange={vi.fn()}
      onModelChange={vi.fn()}
      onProjectChange={vi.fn()}
      onRuntimeModeChange={vi.fn()}
      onSend={vi.fn()}
      projects={createDefaultProjects()}
      runtimeMode="cloud"
      selectedModel="model-1"
      showProjectSelector={false}
      {...overrides}
    />,
  );
}

function renderControlledComposer({
  initialDraft = "",
  ...overrides
}: Partial<React.ComponentProps<typeof ChatComposer>> & { initialDraft?: string } = {}) {
  function ControlledComposer() {
    const [draft, setDraft] = useState(initialDraft);

    return (
      <ChatComposer
        activeProjectId={UNBOUND_PROJECT_ID}
        disabled={false}
        draft={draft}
        isStreaming={false}
        models={[{ id: "model-1", name: "Model 1" }]}
        onCancel={vi.fn()}
        onDraftChange={setDraft}
        onModelChange={vi.fn()}
        onProjectChange={vi.fn()}
        onRuntimeModeChange={vi.fn()}
        onSend={vi.fn()}
        projects={createDefaultProjects()}
        runtimeMode="cloud"
        selectedModel="model-1"
        showProjectSelector={false}
        {...overrides}
      />
    );
  }

  return render(<ControlledComposer />);
}

function renderControlledComposerWithRebuiltHistory(texts: string[]) {
  function ControlledComposer() {
    const [draft, setDraft] = useState("");

    return (
      <ChatComposer
        activeProjectId={UNBOUND_PROJECT_ID}
        disabled={false}
        draft={draft}
        inputHistory={committedHistory(...texts)}
        isStreaming={false}
        models={[{ id: "model-1", name: "Model 1" }]}
        onCancel={vi.fn()}
        onDraftChange={setDraft}
        onModelChange={vi.fn()}
        onProjectChange={vi.fn()}
        onRuntimeModeChange={vi.fn()}
        onSend={vi.fn()}
        projects={createDefaultProjects()}
        runtimeMode="cloud"
        selectedModel="model-1"
        showProjectSelector={false}
      />
    );
  }

  return render(<ControlledComposer />);
}

function getComposerInput() {
  return screen.getByRole("textbox") as HTMLTextAreaElement;
}

function moveCursorToEnd(textarea: HTMLTextAreaElement) {
  textarea.setSelectionRange(textarea.value.length, textarea.value.length);
}

function committedHistory(...texts: string[]): SessionInputHistory {
  return {
    committed: texts.map((text, index) => ({
      messageId: `msg-${index + 1}`,
      itemIndex: 0,
      text,
    })),
  };
}

describe("ChatComposer", () => {
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

  it("allows stopping an active response even when the composer is otherwise disabled", () => {
    const onCancel = vi.fn();
    renderComposer({
      disabled: true,
      isStreaming: true,
      onCancel,
    });

    const stopButton = screen.getByRole("button", { name: "Stop response" });
    expect((stopButton as HTMLButtonElement).disabled).toBe(false);

    fireEvent.click(stopButton);

    expect(onCancel).toHaveBeenCalledTimes(1);
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
