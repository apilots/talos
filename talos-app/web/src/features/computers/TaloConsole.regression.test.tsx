// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import userEvent from "@testing-library/user-event";
import { StrictMode, useState } from "react";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { MemoryRouter } from "react-router-dom";
import { expect, it, vi } from "vitest";

import { ChatSessionMessageActions } from "@/pages/chat/ChatSessionStatusPanels";
import { TaloConsole } from "./TaloConsole";
import { listFiles, query, setupTaloConsoleTests, showConsole, taloInstance } from "./TaloConsole.test-support";

vi.mock("@/shared/api/desktop-client", () => ({ isDesktopRuntime: () => false, getDesktopBridge: () => undefined }));
setupTaloConsoleTests();

it.each([
  ["Media", "text/plain", "image/png"],
  ["Files", "image/png", "text/plain"],
] as const)("loads matching %s from a later page when the first page has no matches", async (filter, firstMime, nextMime) => {
  listFiles
    .mockResolvedValueOnce({
      data: [{ fileId: "first", originalFilename: "first-file", mimeType: firstMime }],
      nextCursor: "next-page",
    })
    .mockResolvedValueOnce({
      data: [{ fileId: "next", originalFilename: "matching-file", mimeType: nextMime }],
      nextCursor: null,
    });
  showConsole();
  fireEvent.click(await screen.findByRole("button", { name: "Open Talo controls" }));
  fireEvent.click(screen.getByRole("tab", { name: "Files" }));
  await screen.findByRole("button", { name: "first-file" });
  fireEvent.click(screen.getByRole("button", { name: filter }));
  expect(within(screen.getByRole("tabpanel")).getByRole("status")).toHaveTextContent("No matching files loaded yet.");
  fireEvent.click(screen.getByRole("button", { name: "Load more files" }));
  expect(await screen.findByRole("button", { name: "matching-file" })).toBeVisible();
  expect(listFiles).toHaveBeenLastCalledWith({ cursor: "next-page", limit: 50 });
  expect(screen.queryByRole("button", { name: "Load more files" })).not.toBeInTheDocument();
});

it.each(["message", "console"] as const)("restores the %s model launcher after button and Escape closes", async (source) => {
  function MessageConsole() {
    const [action, setAction] = useState<{ id: number; kind: "models" } | null>(null);
    return <>
      <ChatSessionMessageActions taloMode onOpenSettings={() => setAction((previous) => ({ id: (previous?.id ?? 0) + 1, kind: "models" }))} />
      <TaloConsole initialInstance={taloInstance} actionRequest={action} />
    </>;
  }
  const user = userEvent.setup();
  render(<StrictMode><QueryClientProvider client={query}><MemoryRouter><MessageConsole /></MemoryRouter></QueryClientProvider></StrictMode>);
  expect(document.body).toHaveFocus();
  await screen.findByRole("button", { name: "Talo · Talo is ready" });
  if (source === "console") {
    await user.click(screen.getByRole("button", { name: "Open Talo controls" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "Close Talo controls" })).toHaveFocus());
  }
  const launcher = screen.getByRole("button", { name: source === "message" ? "Open Settings" : "Model configuration" });
  launcher.focus();
  await user.keyboard("{Enter}");
  let dialog = await screen.findByRole("dialog", { name: "Model configuration" });
  await user.click(within(dialog).getByRole("button", { name: "Close model configuration" }));
  await waitFor(() => expect(launcher).toHaveFocus());
  await user.keyboard("{Enter}");
  dialog = await screen.findByRole("dialog", { name: "Model configuration" });
  expect(dialog).toContainElement(document.activeElement as HTMLElement);
  await user.keyboard("{Escape}");
  await waitFor(() => expect(launcher).toHaveFocus());
});
