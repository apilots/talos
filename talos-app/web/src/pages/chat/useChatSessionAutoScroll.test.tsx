// @vitest-environment jsdom

import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import { useChatSessionAutoScroll } from "./useChatSessionAutoScroll";
import type { ChatRenderBlock } from "@/shared/types/ui";

describe("useChatSessionAutoScroll", () => {
  afterEach(cleanup);

  it("anchors the current user message until the user returns to the bottom", async () => {
    const emptyBlocks: ChatRenderBlock[] = [];
    const { result, rerender } = renderHook(
      ({ blocks }: { blocks: ChatRenderBlock[] }) =>
        useChatSessionAutoScroll({ blocks, isStreaming: true, sessionId: "session-1" }),
      { initialProps: { blocks: emptyBlocks } },
    );
    const viewport = document.createElement("div");
    const content = document.createElement("div");
    const user = document.createElement("div");
    user.dataset.chatUserBlockId = "user-message-1";
    content.appendChild(user);
    viewport.appendChild(content);
    let scrollTop = 250;
    let maxScrollTop = 300;
    Object.defineProperty(viewport, "scrollTop", {
      configurable: true,
      get: () => scrollTop,
      set: (value: number) => {
        scrollTop = Math.min(value, maxScrollTop);
      },
    });
    viewport.getBoundingClientRect = () => rect(0);
    user.getBoundingClientRect = () => rect(610 - viewport.scrollTop);
    result.current.viewportRef.current = viewport;
    result.current.contentRef.current = content;

    const userBlock: ChatRenderBlock = {
      id: "user-message-1",
      kind: "user",
      text: "Keep this prompt stable.",
      fileIds: [],
      live: false,
    };
    rerender({ blocks: [userBlock] });
    expect(viewport.scrollTop).toBe(300);

    const responseBlock: ChatRenderBlock = {
      id: "timeline-session-1-message-1",
      kind: "timeline",
      items: [],
      live: true,
    };
    maxScrollTop = 590;
    rerender({ blocks: [userBlock, responseBlock] });

    expect(viewport.scrollTop).toBe(590);
    await act(async () => {
      await new Promise<void>((resolve) => window.requestAnimationFrame(() => resolve()));
    });

    Object.defineProperty(viewport, "scrollHeight", { configurable: true, value: 990 });
    Object.defineProperty(viewport, "clientHeight", { configurable: true, value: 400 });
    act(() => result.current.handleScroll(viewport));
    rerender({
      blocks: [
        userBlock,
        responseBlock,
        { ...responseBlock, id: "timeline-session-1-message-delayed-scroll" },
      ],
    });

    expect(viewport.scrollTop).toBe(590);

    Object.defineProperty(viewport, "scrollHeight", { configurable: true, value: 1_000 });
    maxScrollTop = 1_000;
    viewport.scrollTop = 600;
    act(() => result.current.handleScroll(viewport));
    rerender({
      blocks: [
        userBlock,
        responseBlock,
        { ...responseBlock, id: "timeline-session-1-message-2" },
      ],
    });

    expect(viewport.scrollTop).toBe(1_000);
  });

  it("opens historical content at the bottom", () => {
    const emptyBlocks: ChatRenderBlock[] = [];
    const { result, rerender } = renderHook(
      ({ blocks }: { blocks: ChatRenderBlock[] }) =>
        useChatSessionAutoScroll({ blocks, isStreaming: false, sessionId: "session-1" }),
      { initialProps: { blocks: emptyBlocks } },
    );
    const viewport = document.createElement("div");
    Object.defineProperty(viewport, "scrollHeight", { configurable: true, value: 900 });
    result.current.viewportRef.current = viewport;

    const historicalBlocks: ChatRenderBlock[] = [
      {
        id: "user-message-1",
        kind: "user",
        text: "Historical prompt.",
        fileIds: [],
        live: false,
      },
    ];
    act(() => {
      rerender({ blocks: historicalBlocks });
    });

    expect(viewport.scrollTop).toBe(900);
  });
});

function rect(top: number): DOMRect {
  return {
    bottom: top,
    height: 0,
    left: 0,
    right: 0,
    top,
    width: 0,
    x: 0,
    y: top,
    toJSON: () => ({}),
  };
}
