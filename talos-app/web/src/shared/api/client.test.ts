import { afterEach, describe, expect, it, vi } from "vitest";

import {
  getTalosClient,
  resetTalosClientFactory,
  setTalosClientFactory,
  type TalosWebClient,
} from "./client";

describe("Talos client runtime factory", () => {
  afterEach(() => {
    resetTalosClientFactory();
  });

  it("allows desktop runtime to inject a transport-specific client", () => {
    const client = { marker: "desktop" } as unknown as TalosWebClient;
    const factory = vi.fn(() => client);

    setTalosClientFactory(factory);

    expect(getTalosClient()).toBe(client);
    expect(factory).toHaveBeenCalledTimes(1);
  });
});
