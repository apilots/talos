// @vitest-environment jsdom

import { renderHook, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import {
  hasInitialSessionInput,
  useInitialSessionInput,
} from "./useInitialSessionInput";

describe("useInitialSessionInput", () => {
  it("sends an attachment-only navigation draft once", async () => {
    const locationState = {
      initialPrompt: "",
      initialPermissionMode: "approveForMe" as const,
      initialAttachments: [
        {
          fileId: "file-1",
          scope: { type: "personal" as const },
          originalFilename: "diagram.png",
          mimeType: "image/png",
          sizeBytes: 8,
          tags: [],
          metadata: {},
          createdAt: 1,
          updatedAt: 1,
        },
      ],
    };
    const sendText = vi.fn(() => new Promise<void>(() => undefined));
    const navigate = vi.fn();

    renderHook(() =>
      useInitialSessionInput({
        onPendingSessionChange: vi.fn(),
        locationPathname: "/sessions/sess-1",
        locationState,
        navigate,
        projects: [],
        sendText,
        sessionId: "sess-1",
        setDraft: vi.fn(),
      }),
    );

    await waitFor(() => {
      expect(sendText).toHaveBeenCalledWith("", {
        modelSelection: undefined,
        userPermissionMode: "approveForMe",
        project: undefined,
        fileIds: ["file-1"],
      });
    });
    expect(hasInitialSessionInput(locationState)).toBe(true);
    expect(navigate).toHaveBeenCalledWith("/sessions/sess-1", { replace: true, state: null });
  });
});
