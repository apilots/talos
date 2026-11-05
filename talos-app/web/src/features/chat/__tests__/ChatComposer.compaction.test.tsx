// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import "@/shared/i18n/setup";
import { act, cleanup, fireEvent, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { FileInfo } from "@talos/sdk-browser";
import { ChatComposer } from "../components/ChatComposer";
import { useChatSession } from "../hooks/useChatSession";
import { useAppStore } from "@/shared/store/app-store";
import { createDefaultProjects, UNBOUND_PROJECT_ID } from "@/shared/types/domain";
import { automationController } from "@/features/automation/automation.test-support";
import { applyStreamEvent } from "../stream-events";
import { contextCompactionStarted, contextCompactionCompleted } from "./streaming-fixtures";
import { getComposerInput, renderWithQueryClient, setComposerValue } from "./ChatComposer.test-utils";
const backend = vi.hoisted(() => ({ add: vi.fn(), start: vi.fn(), streamText: vi.fn() }));
vi.mock("../api", async (original) => ({ ...await original<typeof import("../api")>(), suggestSkills: vi.fn().mockResolvedValue([]) }));
vi.mock("../hooks/useChatQueries", async (original) => ({
  ...await original<typeof import("../hooks/useChatQueries")>(),
  useActiveTurnQuery: () => ({ data: { activeTurn: { identity: { sessionId: "sess-1", turnId: "turn-1" }, state: "running" } }, isSuccess: true, isFetchedAfterMount: true, refetch: vi.fn() }),
  useSessionHistoryQuery: () => ({ data: { data: [], nextCursor: null, lastStableEventId: null }, isSuccess: true, isFetchedAfterMount: true, refetch: vi.fn() }),
}));
vi.mock("../hooks/useChatSessionTimelineLifecycle", () => ({
  isLiveRecoverableActiveTurn: (state: string) => state === "running",
  useChatSessionTimelineLifecycle: () => ({ retryTimeline: vi.fn(), setCompactionObservationActive: vi.fn() }),
}));
vi.mock("../hooks/useSessionMessageQueue", () => ({
  useSessionMessageQueue: () => ({ add: backend.add, update: vi.fn(), delete: vi.fn(), reorder: vi.fn(), start: vi.fn(), steer: vi.fn() }),
}));
vi.mock("../hooks/useSessionStreaming", () => ({
  useSessionStreaming: () => ({ sendMutation: { mutateAsync: backend.start }, sendText: backend.streamText, stopResponse: vi.fn(), isStreaming: true, isCancelling: false, canRetryResponse: false }),
}));
const attachment: FileInfo = { fileId: "notes-file", scope: { type: "personal" }, originalFilename: "notes.txt", mimeType: "text/plain", sizeBytes: 10, tags: [], metadata: {}, createdAt: 1, updatedAt: 1 };
function Harness() {
  const chat = useChatSession("sess-1", 1);
  return <ChatComposer draft={chat.draft} onDraftChange={chat.setDraft}
    onSend={(attachments, mode) => chat.sendTurn({ fileIds: attachments.map((file) => file.fileId), skillSelection: mode ? { name: mode, path: `/worker/system/${mode}/SKILL.md` } : undefined })}
    models={[]} projects={createDefaultProjects()} activeProjectId={UNBOUND_PROJECT_ID} onProjectChange={vi.fn()} showProjectSelector={false}
    isStreaming automation={automationController()} supportedAttachmentKinds={["file"]} initialAttachments={[attachment]} />;
}
beforeEach(() => {
  useAppStore.setState({ streams: {}, composerDrafts: {} });
  backend.add.mockReset().mockResolvedValue(undefined); backend.start.mockClear(); backend.streamText.mockClear();
  applyStreamEvent("sess-1", contextCompactionStarted("compact-1", "summarizing", null, 1));
});
afterEach(cleanup);
it("retains draft, mode and attachments when the real send hook rejects automatic compaction, then queues once after it finishes", async () => {
  renderWithQueryClient(<Harness />);
  setComposerValue("Fix tests");
  fireEvent.click(screen.getByRole("button", { name: "Add" }));
  fireEvent.click(screen.getByRole("button", { name: "Goal" }));
  fireEvent.keyDown(getComposerInput(), { key: "Enter" });
  await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent("Context is compacting"));
  expect(getComposerInput()).toHaveValue("Fix tests");
  expect(screen.getByTestId("automation-mode-chip")).toHaveTextContent("Goal");
  expect(screen.getByText("notes.txt")).toBeInTheDocument();
  expect(backend.add).not.toHaveBeenCalled(); expect(backend.start).not.toHaveBeenCalled();
  act(() => applyStreamEvent("sess-1", contextCompactionCompleted("compact-1", 2, "applied", "Compaction complete")));
  fireEvent.keyDown(getComposerInput(), { key: "Enter" });
  await waitFor(() => expect(backend.add).toHaveBeenCalledWith({ content: [
    { type: "skillSelection", name: "goal", path: "/worker/system/goal/SKILL.md" },
    { type: "text", text: "Fix tests" }, { type: "file", fileId: "notes-file" },
  ] }));
  await waitFor(() => expect(screen.queryByTestId("automation-mode-chip")).not.toBeInTheDocument());
  expect(screen.queryByText("notes.txt")).not.toBeInTheDocument();
  expect(getComposerInput()).toHaveValue("");
});
