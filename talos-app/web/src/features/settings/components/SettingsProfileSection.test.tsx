// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import "@/shared/i18n/setup";
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { SettingsProfileSection } from "./SettingsProfileSection";

const mocks = vi.hoisted(() => ({
  sessions: [] as Array<{
    id: string;
    model_ref: { id: string };
    usage: { total_tokens: number; cache_read_input_tokens?: number };
    message_count: number;
    last_activity: number;
  }>,
  projects: [] as Array<{ id: string; name: string }>,
}));

vi.mock("@/features/chat", () => ({
  useProjectsView: () => ({ projects: mocks.projects }),
  useSessionsQuery: () => ({ data: mocks.sessions }),
}));

describe("SettingsProfileSection", () => {
  afterEach(() => {
    cleanup();
    mocks.sessions = [];
    mocks.projects = [];
  });

  it("does not render synthetic model usage when no sessions exist", () => {
    render(
      <SettingsProfileSection
        connection={{
          authStatus: "open",
          authToken: "",
          authRole: "user",
          accountType: "Free",
          authUsername: "Anonymous",
          userId: "anonymous",
          runtimeMode: "local",
        }}
      />,
    );

    expect(screen.getAllByText("No model usage yet.")).toHaveLength(2);
    expect(screen.queryByText("qwen3.6-flash")).toBeNull();
    expect(screen.queryByText("3 runs")).toBeNull();
    expect(screen.queryByText("63%")).toBeNull();
    expect(screen.getByText("Open")).toBeTruthy();
  });
});
