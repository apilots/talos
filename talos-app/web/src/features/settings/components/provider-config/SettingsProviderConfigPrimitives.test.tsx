// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import {
  SettingsConfigSaveFooter,
  SettingsProviderKeyField,
  SettingsProviderTabs,
} from "./SettingsProviderConfigPrimitives";

describe("SettingsProviderConfigPrimitives", () => {
  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
  });

  it("selects a provider tab and marks the active tab", () => {
    const onChange = vi.fn();

    render(
      <SettingsProviderTabs
        activeProvider="firecrawl"
        onChange={onChange}
        providers={[
          { key: "firecrawl", label: "Firecrawl" },
          { key: "brave", label: "Brave" },
        ]}
      />,
    );

    expect(screen.getByRole("button", { name: "Firecrawl" }).className).toContain("text-ink");
    fireEvent.click(screen.getByRole("button", { name: "Brave" }));

    expect(onChange).toHaveBeenCalledWith("brave");
  });

  it("shows configured key state and supports clearing", () => {
    const onChange = vi.fn();
    const onClear = vi.fn();

    render(
      <SettingsProviderKeyField
        clearLabel="Clear API Key"
        configured
        label="API Key"
        onChange={onChange}
        onClear={onClear}
        value=""
      />,
    );

    fireEvent.change(screen.getByLabelText("API Key"), { target: { value: "new-secret" } });
    fireEvent.click(screen.getByRole("button", { name: "Clear API Key" }));

    expect(onChange).toHaveBeenCalledWith("new-secret");
    expect(onClear).toHaveBeenCalledTimes(1);
  });

  it("shows dirty and pending footer states", () => {
    const onSave = vi.fn();

    const { rerender } = render(
      <SettingsConfigSaveFooter
        dirty
        error={null}
        onSave={onSave}
        parseError={(error) => String(error)}
        pending={false}
        saveLabel="Save Changes"
        savingLabel="Saving..."
        unsavedLabel="Unsaved changes"
      >
        Current on next turn
      </SettingsConfigSaveFooter>,
    );

    expect(screen.getByText("Unsaved changes")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Save Changes" }));
    expect(onSave).toHaveBeenCalledTimes(1);

    rerender(
      <SettingsConfigSaveFooter
        dirty
        error={null}
        onSave={onSave}
        parseError={(error) => String(error)}
        pending
        saveLabel="Save Changes"
        savingLabel="Saving..."
        unsavedLabel="Unsaved changes"
      />,
    );

    expect(screen.getByRole("button", { name: "Saving..." })).toHaveProperty("disabled", true);
  });

  it("shows success and error footer states", () => {
    const { rerender } = render(
      <SettingsConfigSaveFooter
        dirty={false}
        error={null}
        onSave={vi.fn()}
        parseError={(error) => String(error)}
        pending={false}
        saveLabel="Save Changes"
        savingLabel="Saving..."
        unsavedLabel="Unsaved changes"
      >
        Current on next turn
      </SettingsConfigSaveFooter>,
    );

    expect(screen.getByText("Current on next turn")).toBeTruthy();

    rerender(
      <SettingsConfigSaveFooter
        dirty={false}
        error={new Error("Save failed")}
        onSave={vi.fn()}
        parseError={(error) => (error instanceof Error ? error.message : "Request failed")}
        pending={false}
        saveLabel="Save Changes"
        savingLabel="Saving..."
        unsavedLabel="Unsaved changes"
      />,
    );

    expect(screen.getByText("Save failed")).toBeTruthy();
  });
});
