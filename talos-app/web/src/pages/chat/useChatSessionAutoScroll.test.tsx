// @vitest-environment jsdom
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { useChatSessionAutoScroll } from "./useChatSessionAutoScroll";
import type { ChatRenderBlock } from "@/shared/types/ui";

describe("useChatSessionAutoScroll", () => {
  afterEach(cleanup);
  it("follows execution, respects reading history, and follows a newly sent Turn", () => {
    const { result, rerender } = renderHook(
      ({ streaming, blocks }) => useChatSessionAutoScroll({
        blocks, isStreaming: streaming, sessionId: "session-1",
      }),
      { initialProps: { streaming: false, blocks: [] as ChatRenderBlock[] } },
    );
    const viewport = document.createElement("div");
    Object.defineProperty(viewport, "scrollHeight", { configurable: true, value: 900 });
    Object.defineProperty(viewport, "clientHeight", { value: 400 });
    result.current.viewportRef.current = viewport;
    const blocks: ChatRenderBlock[] = [{
      id: "user-1", kind: "user", text: "Prompt", fileIds: [], live: true,
    }];
    rerender({ streaming: true, blocks });
    expect(viewport.scrollTop).toBe(900);
    Object.defineProperty(viewport, "scrollHeight", { configurable: true, value: 1200 });
    rerender({ streaming: true, blocks: [...blocks] });
    expect(viewport.scrollTop).toBe(1200);
    viewport.scrollTop = 100;
    act(() => result.current.handleScroll(viewport));
    rerender({ streaming: true, blocks: [...blocks] });
    expect(viewport.scrollTop).toBe(100);
    rerender({ streaming: false, blocks });
    expect(viewport.scrollTop).toBe(100);
    rerender({ streaming: true, blocks: [...blocks] });
    expect(viewport.scrollTop).toBe(1200);
  });
});
