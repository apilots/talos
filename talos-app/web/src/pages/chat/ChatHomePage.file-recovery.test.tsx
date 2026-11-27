import { restoreComposerRequest } from "@/features/chat";
// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import "@/shared/i18n/setup";
import { fireEvent, screen } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import { Link, MemoryRouter, Route, Routes } from "react-router-dom";
import { createBrowserTalosClient, type FileInfo } from "@talos/sdk-browser";
import { setTalosClientFactory, type TalosWebClient } from "@/shared/api/client";
import { useAppStore } from "@/shared/store/app-store";
import { render, composer } from "./ChatHomePage.test-support";
import { ChatHomePage } from "./ChatHomePage";

it.each(["unchanged", "retry", "caption", "picker", "remove"] as const)(
  "retains restored file references across partial metadata failure (%s)", async (recovery) => {
    composer.real = true;
    const files: FileInfo[] = ["first", "second"].map(name => ({
      fileId: `file-${name}`, originalFilename: `${name}.png`, mimeType: "image/png", sizeBytes: 8,
      scope: { type: "personal" }, tags: [], metadata: {}, createdAt: 1, updatedAt: 1,
    }));
    let serviceUp = false;
    const read = vi.fn(({ fileId }: { fileId: string }) => {
      if (fileId === "file-second" && !serviceUp) return Promise.reject(new Error("Metadata temporarily unavailable"));
      return Promise.resolve({ file: files.find(file => file.fileId === fileId)! });
    });
    const sent: Array<{ key: string | null; body: string }> = [];
    const accepted = new Set<string | null>();
    const http = vi.fn<typeof fetch>( (url, init) => {
      if (!(url instanceof Request ? url.url : url.toString()).endsWith("/v1/turns/start")) return Promise.resolve(Response.json({ status: "success", data: { data: [] } }));
      const submission = { key: new Headers(init?.headers).get("idempotency-key"), body: (typeof init?.body === "string" ? init.body : (() => { throw new Error("Expected a JSON request body."); })()) };
      sent.push(submission); accepted.add(submission.key);
      if (sent.length === 1) return Promise.reject(new TypeError("Response lost after admission"));
      return Promise.resolve(Response.json({ status: "success", data: { turnId: "turn-1", messageId: "message-1" } }));
    });
    setTalosClientFactory(() => ({ ...createBrowserTalosClient({ baseUrl: "https://server.example", fetch: http }),
      files: { read, list: vi.fn().mockResolvedValue({ data: files, nextCursor: null }),
        upload: vi.fn((_request: unknown, file: File) => Promise.resolve(({ file: files.find(item => item.originalFilename === file.name)! }))) },
    }) as unknown as TalosWebClient);
    const view = render(<MemoryRouter><Routes>
      <Route path="/" element={<><ChatHomePage /><Link to="/settings">Settings</Link></>} />
      <Route path="/settings" element={<Link to="/">Return Home</Link>} />
      <Route path="/sessions/:sessionId" element={<div>Admitted conversation</div>} />
    </Routes></MemoryRouter>);
    fireEvent.change(view.container.querySelector('input[type="file"]')!, { target: { files: [
      new File(["first"], "first.png", { type: "image/png" }), new File(["second"], "second.png", { type: "image/png" }),
    ] } });
    await screen.findByRole("button", { name: "Remove first.png" });
    await screen.findByRole("button", { name: "Remove second.png" });
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
    await screen.findByRole("alert");
    expect(restoreComposerRequest(sent[0].body).input.content).toEqual([
      { type: "file", fileId: "file-first" }, { type: "file", fileId: "file-second" },
    ]);
    fireEvent.click(screen.getByRole("link", { name: "Settings" }));
    fireEvent.click(screen.getByRole("link", { name: "Return Home" }));
    await screen.findByRole("button", { name: "Remove first.png" });
    await screen.findByRole("button", { name: "Retry now file-second" });
    expect(screen.getByRole("button", { name: "Remove file-second" })).toBeVisible();
    expect(screen.getByRole("button", { name: "Send" })).toBeEnabled();

    if (recovery === "retry") {
      serviceUp = true;
      fireEvent.click(screen.getByRole("button", { name: "Retry now file-second" }));
      await screen.findByRole("button", { name: "Remove second.png" });
      expect(read.mock.calls.filter(([request]) => request.fileId === "file-second")).toHaveLength(2);
    } else if (recovery === "picker") {
      serviceUp = true;
      fireEvent.click(screen.getByRole("button", { name: "Add" }));
      fireEvent.click(screen.getByRole("button", { name: "Choose an existing file" }));
      fireEvent.click(await screen.findByText("second.png"));
      await screen.findByRole("button", { name: "Remove second.png" });
    } else if (recovery === "caption") {
      fireEvent.change(screen.getByRole("textbox"), { target: { value: "Please review both images" } });
    } else if (recovery === "remove") {
      fireEvent.click(screen.getByRole("button", { name: "Remove file-second" }));
    }
    if (recovery === "unchanged" || recovery === "retry" || recovery === "picker") {
      const key = JSON.stringify(["test-scope", "embedded", "home"]);
      expect(useAppStore.getState().composerRequests[key].requestId).toBe(sent[0].key);
    }
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
    await screen.findByText("Admitted conversation");
    expect(sent).toHaveLength(2);
    if (recovery === "caption" || recovery === "remove") {
      expect(sent[1].key).not.toBe(sent[0].key);
      expect(restoreComposerRequest(sent[1].body).input.content).toEqual([
        ...(recovery === "caption" ? [{ type: "text", text: "Please review both images" }] : []),
        { type: "file", fileId: "file-first" },
        ...(recovery === "caption" ? [{ type: "file", fileId: "file-second" }] : []),
      ]);
      expect(accepted.size).toBe(2);
    } else {
      expect(sent[1]).toEqual(sent[0]);
      expect(accepted.size).toBe(1);
    }
  },
);
