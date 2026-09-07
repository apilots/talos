import { useState } from "react";
import type React from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
import { vi } from "vitest";

import { ChatComposer } from "../components/ChatComposer";
import { createDefaultProjects, UNBOUND_PROJECT_ID } from "@/shared/types/domain";
import type { SessionInputHistory } from "@/shared/types/ui";

export function renderComposer(overrides: Partial<React.ComponentProps<typeof ChatComposer>> = {}) {
  return renderWithQueryClient(
    <ChatComposer {...defaultComposerProps()} {...overrides} />,
  );
}

export function renderControlledComposer({
  initialDraft = "",
  ...overrides
}: Partial<React.ComponentProps<typeof ChatComposer>> & { initialDraft?: string } = {}) {
  function ControlledComposer() {
    const [draft, setDraft] = useState(initialDraft);

    return (
      <ChatComposer
        {...defaultComposerProps()}
        draft={draft}
        onDraftChange={setDraft}
        {...overrides}
      />
    );
  }

  return renderWithQueryClient(<ControlledComposer />);
}

export function renderControlledComposerWithRebuiltHistory(texts: string[]) {
  function ControlledComposer() {
    const [draft, setDraft] = useState("");

    return (
      <ChatComposer
        {...defaultComposerProps()}
        draft={draft}
        inputHistory={committedHistory(...texts)}
        onDraftChange={setDraft}
      />
    );
  }

  return renderWithQueryClient(<ControlledComposer />);
}

function defaultComposerProps(): React.ComponentProps<typeof ChatComposer> {
  return {
    activeProjectId: UNBOUND_PROJECT_ID,
    draft: "",
    isStreaming: false,
    models: [{ id: "mock:model-1", name: "Model 1", provider: "mock" }],
    onCancel: vi.fn(),
    onDraftChange: vi.fn(),
    onModelChange: vi.fn(),
    onProjectChange: vi.fn(),
    onSend: vi.fn(),
    projects: createDefaultProjects(),
    selectedModel: "mock:model-1",
    showProjectSelector: false,
  };
}

export function renderWithQueryClient(element: React.ReactElement) {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      {element}
    </QueryClientProvider>,
  );
}

export function getComposerInput() {
  return screen.getByRole("textbox") as HTMLTextAreaElement;
}

export function getComposerValue(editor = getComposerInput()) {
  return editor.value;
}

export function setComposerValue(value: string, editor = getComposerInput()) {
  editor.focus();
  fireEvent.change(editor, { target: { value } });
  editor.setSelectionRange(value.length, value.length);
  fireEvent.keyUp(editor);
}

export function setComposerSelection(cursorIndex: number, editor = getComposerInput()) {
  editor.focus();
  editor.setSelectionRange(cursorIndex, cursorIndex);
  fireEvent.keyUp(editor);
}

export function moveCursorToEnd(editor: HTMLTextAreaElement) {
  setComposerSelection(getComposerValue(editor).length, editor);
}

export function committedHistory(...texts: string[]): SessionInputHistory {
  return {
    committed: texts.map((text, index) => ({
      messageId: `msg-${index + 1}`,
      itemIndex: 0,
      text,
      fileIds: [],
    })),
  };
}
