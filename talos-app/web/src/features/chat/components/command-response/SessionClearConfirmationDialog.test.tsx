// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import "@/shared/i18n/setup";
import { SessionClearConfirmationDialog } from "./SessionClearConfirmationDialog";

describe("SessionClearConfirmationDialog", () => {
  afterEach(cleanup);

  it("uses the Talos dialog surface and protects the unsent draft", () => {
    const onCancel = vi.fn();
    const onConfirm = vi.fn();
    render(
      <SessionClearConfirmationDialog
        clearing={false}
        onCancel={onCancel}
        onConfirm={onConfirm}
        open
      />,
    );

    expect(screen.getByRole("alertdialog", { name: "Clear current session?" })).toHaveTextContent(
      "Your unsent draft and attachments will remain unchanged.",
    );
    fireEvent.click(screen.getByRole("button", { name: "Clear session" }));
    expect(onConfirm).toHaveBeenCalledTimes(1);
  });
});
