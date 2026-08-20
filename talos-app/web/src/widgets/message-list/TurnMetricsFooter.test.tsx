// @vitest-environment jsdom

import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import "@/shared/i18n/setup";
import type { ChatTurnMetrics } from "@/shared/types/ui";
import { TurnMetricsFooter } from "./TurnMetricsFooter";

const metrics: ChatTurnMetrics = {
  state: "completed",
  model_rounds: 11,
  tool_calls: 16,
  tool_successes: 16,
  tool_errors: 0,
  stop_reason: "end_turn",
  usage: {
    input_tokens: 900_000,
    output_tokens: 78_200,
    total_tokens: 978_200,
    cache_creation_input_tokens: null,
    cache_read_input_tokens: null,
  },
  duration_ms: 30_000_000,
};

describe("TurnMetricsFooter", () => {
  afterEach(cleanup);

  it("renders the agreed lightweight metrics in a fixed order", () => {
    const { container } = render(<TurnMetricsFooter metrics={metrics} />);

    const footer = screen.getByLabelText(/Turn average/);
    expect(footer.textContent).toBe(
      "32.61 tokens/s·978.2k tokens·11 rounds·16 tools·0 errors",
    );
    expect(container.querySelectorAll("[data-metric]")).toHaveLength(5);
  });

  it("omits only tokens per second when duration is unavailable", () => {
    render(<TurnMetricsFooter metrics={{ ...metrics, duration_ms: null }} />);

    expect(screen.queryByText(/tokens\/s/)).toBeNull();
    expect(screen.getByText("978.2k tokens")).not.toBeNull();
    expect(screen.getByText("0 errors")).not.toBeNull();
  });

  it("uses singular labels for single-value metrics", () => {
    render(
      <TurnMetricsFooter
        metrics={{ ...metrics, model_rounds: 1, tool_calls: 1, tool_errors: 1 }}
      />,
    );

    expect(screen.getByText("1 round")).not.toBeNull();
    expect(screen.getByText("1 tool")).not.toBeNull();
    expect(screen.getByText("1 error")).not.toBeNull();
  });
});
