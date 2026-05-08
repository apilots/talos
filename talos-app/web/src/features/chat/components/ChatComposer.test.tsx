// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import "@/shared/i18n/setup";
import { createDefaultProjects, UNBOUND_PROJECT_ID } from "@/shared/types/domain";
import { ChatComposer } from "./ChatComposer";

function renderComposer(overrides: Partial<React.ComponentProps<typeof ChatComposer>> = {}) {
  return render(
    <ChatComposer
      activeProjectId={UNBOUND_PROJECT_ID}
      approvalMode="skip"
      draft=""
      isStreaming={false}
      models={[{ id: "model-1", name: "Model 1" }]}
      onApprovalModeChange={vi.fn()}
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

describe("ChatComposer", () => {
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
});
