// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import "@/shared/i18n/setup";
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { PluginCatalogActions } from "./PluginCatalogActions";

describe("PluginCatalogActions", () => {
  it("closes the menu when clicking outside", () => {
    const onMenuOpenChange = vi.fn();

    render(
      <>
        <button type="button">Outside</button>
        <PluginCatalogActions
          activeTab="plugins"
          menuOpen
          onAddMarketplace={vi.fn()}
          onCreatePlugin={vi.fn()}
          onCreateSkill={vi.fn()}
          onMenuOpenChange={onMenuOpenChange}
          onRefresh={vi.fn()}
        />
      </>,
    );

    fireEvent.pointerDown(screen.getByRole("button", { name: "Outside" }));

    expect(onMenuOpenChange).toHaveBeenCalledWith(false);
  });

  it("closes the menu on Escape", () => {
    const onMenuOpenChange = vi.fn();

    render(
      <PluginCatalogActions
        activeTab="skills"
        menuOpen
        onAddMarketplace={vi.fn()}
        onCreatePlugin={vi.fn()}
        onCreateSkill={vi.fn()}
        onMenuOpenChange={onMenuOpenChange}
        onRefresh={vi.fn()}
      />,
    );

    fireEvent.keyDown(document, { key: "Escape" });

    expect(onMenuOpenChange).toHaveBeenCalledWith(false);
  });
});
