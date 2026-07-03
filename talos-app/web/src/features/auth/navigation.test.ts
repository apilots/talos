// @vitest-environment jsdom

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { getDesktopBridge } from "@/shared/api/desktop-client";
import {
  closePreparedExternalWindow,
  prepareExternalAuthWindow,
  redirectToExternalUrl,
} from "./navigation";

vi.mock("@/shared/api/desktop-client", () => ({
  getDesktopBridge: vi.fn(),
}));

const getDesktopBridgeMock = vi.mocked(getDesktopBridge);

describe("redirectToExternalUrl", () => {
  beforeEach(() => {
    getDesktopBridgeMock.mockReset();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("opens a browser tab before async login starts outside the Desktop runtime", () => {
    const openedWindow = {
      closed: false,
      close: vi.fn(),
      location: { assign: vi.fn() },
      opener: {},
    } as unknown as Window;
    const open = vi.spyOn(window, "open").mockReturnValue(openedWindow);

    const preparedWindow = prepareExternalAuthWindow();

    expect(open).toHaveBeenCalledWith("about:blank", "_blank");
    expect(preparedWindow).toBe(openedWindow);
  });

  it("navigates a prepared browser tab outside the Desktop runtime", () => {
    const preparedWindow = {
      closed: false,
      location: { href: "about:blank" },
      opener: {},
    } as unknown as Window;

    redirectToExternalUrl("https://cloud.talos.love/device?session_id=dls_1", preparedWindow);

    expect(preparedWindow.location.href).toBe("https://cloud.talos.love/device?session_id=dls_1");
    expect(preparedWindow.opener).toBeNull();
  });

  it("closes a prepared browser tab when login startup fails", () => {
    const close = vi.fn();
    const preparedWindow = {
      closed: false,
      close,
    } as unknown as Window;

    closePreparedExternalWindow(preparedWindow);

    expect(close).toHaveBeenCalledTimes(1);
  });

  it("opens an external URL through the Desktop bridge when available", () => {
    const openExternal = vi.fn();
    getDesktopBridgeMock.mockReturnValue({
      invoke: vi.fn(),
      onEvent: () => () => undefined,
      onSubscriptionTerminated: () => () => undefined,
      openExternal,
    });

    redirectToExternalUrl("https://cloud.talos.love/device?session_id=dls_1");

    expect(openExternal).toHaveBeenCalledWith("https://cloud.talos.love/device?session_id=dls_1");
  });
});
