// @vitest-environment jsdom

import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import "@/shared/i18n/setup";
import type { ChatTurnMetrics } from "@/shared/types/ui";
import { TurnMetricsFooter } from "./TurnMetricsFooter";

const metrics: ChatTurnMetrics = {
  state: "completed",
  modelRounds: 11,
  toolCalls: 16,
  toolSuccesses: 16,
  toolErrors: 0,
  usage: {
    input_tokens: 900_000,
    output_tokens: 78_200,
    total_tokens: 978_200,
    cache_creation_input_tokens: null,
    cache_read_input_tokens: null,
  },
  context: null,
  durationMs: 30_000_000,
};

describe("TurnMetricsFooter", () => {
  afterEach(cleanup);

  it("renders the agreed lightweight metrics in a fixed order", () => {
    const { container } = render(<TurnMetricsFooter metrics={metrics} />);

    const footer = screen.getByLabelText(/Turn average/);
    expect(footer.textContent).toBe(
      "2.61 tokens/s·978.2k tokens·11 rounds·16 tools·0 errors",
    );
    expect(container.querySelectorAll("[data-metric]")).toHaveLength(5);
  });

  it("omits only tokens per second when duration is unavailable", () => {
    render(<TurnMetricsFooter metrics={{ ...metrics, durationMs: null }} />);

    expect(screen.queryByText(/tokens\/s/)).toBeNull();
    expect(screen.getByText("978.2k tokens")).not.toBeNull();
    expect(screen.getByText("0 errors")).not.toBeNull();
  });

  it("uses singular labels for single-value metrics", () => {
    render(
      <TurnMetricsFooter
        metrics={{ ...metrics, modelRounds: 1, toolCalls: 1, toolErrors: 1 }}
      />,
    );

    expect(screen.getByText("1 round")).not.toBeNull();
    expect(screen.getByText("1 tool")).not.toBeNull();
    expect(screen.getByText("1 error")).not.toBeNull();
  });
});
