// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import "@/shared/i18n/setup";
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { SettingsProfileSection } from "./SettingsProfileSection";

const mocks = vi.hoisted(() => {
  const profile = {
    sessionCount: 0,
    messageCount: 0,
    projectCount: 0,
    usage: {
      input_tokens: 0,
      output_tokens: 0,
      total_tokens: 0,
      cache_read_input_tokens: null,
      cache_creation_input_tokens: null,
    },
    usageAvailable: true,
    usageStartedAt: null,
    asOfDate: "2026-07-16",
    dailyUsage: [],
    modelUsage: [],
  };
  return {
    profile,
    query: {
      data: profile as typeof profile | null,
      error: null as Error | null,
      isLoading: false,
    },
  };
});

vi.mock("../hooks/useProfileQuery", () => ({
  useProfileQuery: () => mocks.query,
}));

describe("SettingsProfileSection", () => {
  afterEach(() => {
    cleanup();
    mocks.query.data = mocks.profile;
    mocks.query.error = null;
    mocks.query.isLoading = false;
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

  it("shows the backend error instead of hiding it", () => {
    mocks.query.data = null;
    mocks.query.error = new Error("Profile store is unavailable");

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

    expect(screen.getByRole("alert")).toHaveTextContent("Profile store is unavailable");
  });
});
