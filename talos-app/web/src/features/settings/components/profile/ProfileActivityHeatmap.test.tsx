// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import { ProfileActivityHeatmap } from "./ProfileActivityHeatmap";

describe("ProfileActivityHeatmap", () => {
  afterEach(cleanup);

  it("switches between daily, weekly, and cumulative activity", () => {
    render(
      <ProfileActivityHeatmap
        activity={[
          { date: "2026-08-18", tokens: 100 },
          { date: "2026-08-19", tokens: 200 },
          { date: "2026-08-20", tokens: 300 },
        ]}
        asOfDate="2026-08-20"
        cumulativeLabel="Cumulative"
        dailyLabel="Daily"
        title="Token activity"
        weeklyLabel="Weekly"
      />,
    );

    const daily = screen.getByRole("button", { name: "Daily" });
    const weekly = screen.getByRole("button", { name: "Weekly" });
    const cumulative = screen.getByRole("button", { name: "Cumulative" });

    expect(daily).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("img", { name: "Daily token activity" })).toBeInTheDocument();

    fireEvent.click(weekly);
    expect(weekly).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("img", { name: "Weekly token activity" })).toBeInTheDocument();

    fireEvent.click(cumulative);
    expect(cumulative).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("img", { name: "Cumulative token activity" })).toBeInTheDocument();
  });
});
