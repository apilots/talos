// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import "@/shared/i18n/setup";
import { UserActionPanel } from "./UserActionPanel";

describe("UserActionPanel", () => {
  afterEach(cleanup);

  it("submits a server option or a free-form question answer", () => {
    const onSubmit = vi.fn();
    render(
      <UserActionPanel
        action={{
          actionId: "action-1",
          callId: "call-1",
          state: "pending",
          params: {
            type: "question",
            params: {
              header: "Release",
              question: "How should this proceed?",
              options: [
                { id: "now", label: "Release now", description: "Publish immediately." },
              ],
            },
          },
        }}
        onSubmit={onSubmit}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: /release now/i }));
    expect(onSubmit).toHaveBeenLastCalledWith({
      type: "question",
      response: { type: "selectedOption", optionId: "now" },
    });

    fireEvent.click(screen.getByRole("button", { name: "Other" }));
    fireEvent.change(screen.getByRole("textbox", { name: "Other" }), {
      target: { value: "Wait until tomorrow" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Submit response" }));
    expect(onSubmit).toHaveBeenLastCalledWith({
      type: "question",
      response: { type: "freeForm", text: "Wait until tomorrow" },
    });
  });

  it("renders and submits only the approval decisions supplied by the server", () => {
    const onSubmit = vi.fn();
    render(
      <UserActionPanel
        action={{
          actionId: "action-2",
          callId: "call-2",
          state: "pending",
          params: {
            type: "sandboxApproval",
            params: {
              toolName: "Bash",
              reason: "riskyCommand",
              presentation: { type: "command", command: "rm output.txt", cwd: "/workspace" },
              availableDecisions: ["allowOnce", "deny", "cancel"],
            },
          },
        }}
        onSubmit={onSubmit}
      />,
    );

    expect(screen.queryByRole("textbox")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Allow once" }));
    expect(onSubmit).toHaveBeenCalledWith({
      type: "sandboxApproval",
      response: "allowOnce",
    });
  });
});
