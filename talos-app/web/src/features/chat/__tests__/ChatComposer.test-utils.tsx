import { useState } from "react";
import type React from "react";
import { render, screen } from "@testing-library/react";
import { vi } from "vitest";

import { ChatComposer } from "../components/ChatComposer";
import { createDefaultProjects, UNBOUND_PROJECT_ID } from "@/shared/types/domain";
import type { SessionInputHistory } from "@/shared/types/ui";

export function renderComposer(overrides: Partial<React.ComponentProps<typeof ChatComposer>> = {}) {
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
      runtimeMode="local"
      selectedModel="model-1"
      showProjectSelector={false}
      {...overrides}
    />,
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
        runtimeMode="local"
        selectedModel="model-1"
        showProjectSelector={false}
        {...overrides}
      />
    );
  }

  return render(<ControlledComposer />);
}

export function renderControlledComposerWithRebuiltHistory(texts: string[]) {
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
        runtimeMode="local"
        selectedModel="model-1"
        showProjectSelector={false}
      />
    );
  }

  return render(<ControlledComposer />);
}

export function getComposerInput() {
  return screen.getByRole("textbox") as HTMLTextAreaElement;
}

export function moveCursorToEnd(textarea: HTMLTextAreaElement) {
  textarea.setSelectionRange(textarea.value.length, textarea.value.length);
}

export function committedHistory(...texts: string[]): SessionInputHistory {
  return {
    committed: texts.map((text, index) => ({
      messageId: `msg-${index + 1}`,
      itemIndex: 0,
      text,
    })),
  };
}
