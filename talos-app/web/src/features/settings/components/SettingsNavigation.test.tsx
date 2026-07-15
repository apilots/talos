// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { MemoryRouter } from "react-router-dom";

import "@/shared/i18n/setup";
import {
  closePreparedExternalWindow,
  getCloudAuthUrl,
  prepareExternalAuthWindow,
  redirectToExternalUrl,
} from "@/features/auth";
import { SettingsNavigation } from "./SettingsNavigation";

vi.mock("@/features/auth", () => ({
  closePreparedExternalWindow: vi.fn(),
  getCloudAuthUrl: vi.fn(),
  prepareExternalAuthWindow: vi.fn(),
  redirectToExternalUrl: vi.fn(),
}));

const closePreparedExternalWindowMock = vi.mocked(closePreparedExternalWindow);
const getCloudAuthUrlMock = vi.mocked(getCloudAuthUrl);
const prepareExternalAuthWindowMock = vi.mocked(prepareExternalAuthWindow);
const redirectToExternalUrlMock = vi.mocked(redirectToExternalUrl);

describe("SettingsNavigation", () => {
  beforeEach(() => {
    getCloudAuthUrlMock.mockResolvedValue("https://cloud.talos.love");
    prepareExternalAuthWindowMock.mockReturnValue(null);
  });

  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
  });

  it("opens the Cloud account page as an external destination", async () => {
    const preparedWindow = { closed: false } as Window;
    prepareExternalAuthWindowMock.mockReturnValue(preparedWindow);

    render(
      <MemoryRouter>
        <SettingsNavigation activeSection="profile" canOpenAccount />
      </MemoryRouter>,
    );

    fireEvent.click(screen.getByRole("button", { name: "Account" }));

    await waitFor(() => {
      expect(redirectToExternalUrlMock).toHaveBeenCalledWith(
        "https://cloud.talos.love/account",
        preparedWindow,
      );
    });
    expect(getCloudAuthUrlMock).toHaveBeenCalledTimes(1);
    expect(closePreparedExternalWindowMock).not.toHaveBeenCalled();
  });

  it("keeps the account entry disabled for open mode", () => {
    render(
      <MemoryRouter>
        <SettingsNavigation activeSection="profile" canOpenAccount={false} />
      </MemoryRouter>,
    );

    const accountButton = screen.getByRole("button", { name: "Account" });
    expect(accountButton).toHaveProperty("disabled", true);
    fireEvent.click(accountButton);

    expect(prepareExternalAuthWindowMock).not.toHaveBeenCalled();
    expect(redirectToExternalUrlMock).not.toHaveBeenCalled();
  });

  it("shows an error and closes the prepared window when Cloud account cannot open", async () => {
    const preparedWindow = { closed: false } as Window;
    prepareExternalAuthWindowMock.mockReturnValue(preparedWindow);
    getCloudAuthUrlMock.mockRejectedValue(new Error("Cloud URL missing"));
    vi.spyOn(console, "error").mockImplementation(() => undefined);

    render(
      <MemoryRouter>
        <SettingsNavigation activeSection="profile" canOpenAccount />
      </MemoryRouter>,
    );

    fireEvent.click(screen.getByRole("button", { name: "Account" }));

    expect((await screen.findByRole("alert")).textContent).toBe(
      "Cloud account could not be opened.",
    );
    expect(closePreparedExternalWindowMock).toHaveBeenCalledWith(preparedWindow);
    expect(redirectToExternalUrlMock).not.toHaveBeenCalled();
  });
});
