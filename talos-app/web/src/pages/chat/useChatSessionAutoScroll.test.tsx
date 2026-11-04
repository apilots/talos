// @vitest-environment jsdom
import { act, cleanup, render, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useChatSessionAutoScroll } from "./useChatSessionAutoScroll";
import type { ChatRenderBlock } from "@/shared/types/ui";

describe("useChatSessionAutoScroll", () => {
  const frames = new Map<number, FrameRequestCallback>();
  let nextFrame = 0;
  let resize = () => {};
  beforeEach(() => {
    frames.clear();
    nextFrame = 0;
    vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
      frames.set(++nextFrame, callback);
      return nextFrame;
    });
    vi.stubGlobal("cancelAnimationFrame", (id: number) => frames.delete(id));
    vi.stubGlobal("ResizeObserver", class {
      constructor(callback: () => void) { resize = callback; }
      observe() {}
      disconnect() {}
    });
  });
  afterEach(() => {
    cleanup();
    vi.unstubAllGlobals();
  });
  function flushFrame() {
    const callbacks = [...frames.values()];
    frames.clear();
    act(() => callbacks.forEach(callback => callback(0)));
  }
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
    expect(viewport.scrollTop).toBe(0);
    flushFrame();
    expect(viewport.scrollTop).toBe(900);
    Object.defineProperty(viewport, "scrollHeight", { configurable: true, value: 1200 });
    rerender({ streaming: true, blocks: [...blocks] });
    flushFrame();
    expect(viewport.scrollTop).toBe(1200);
    viewport.scrollTop = 100;
    act(() => result.current.handleScroll(viewport));
    rerender({ streaming: true, blocks: [...blocks] });
    flushFrame();
    expect(viewport.scrollTop).toBe(100);
    rerender({ streaming: false, blocks });
    flushFrame();
    expect(viewport.scrollTop).toBe(100);
    rerender({ streaming: true, blocks: [...blocks] });
    flushFrame();
    expect(viewport.scrollTop).toBe(1200);
  });

  it("coalesces resize and content updates, respects a pending scroll-away, and cancels on switching or unmount", () => {
    let scroll!: ReturnType<typeof useChatSessionAutoScroll>;
    function View({ sessionId, blocks }: { sessionId: string; blocks: ChatRenderBlock[] }) {
      scroll = useChatSessionAutoScroll({ blocks, isStreaming: true, sessionId });
      return <div ref={scroll.viewportRef}><div ref={scroll.contentRef} /></div>;
    }
    const view = render(<View sessionId="first" blocks={[]} />);
    const viewport = scroll.viewportRef.current!;
    const height = vi.fn(() => 1200);
    Object.defineProperty(viewport, "scrollHeight", { get: height });
    Object.defineProperty(viewport, "clientHeight", { value: 400 });
    act(() => { resize(); resize(); });
    view.rerender(<View sessionId="first" blocks={[]} />);
    act(() => resize());
    expect(height).not.toHaveBeenCalled();
    expect(frames.size).toBe(1);
    flushFrame();
    expect(height).toHaveBeenCalledTimes(1);
    expect(viewport.scrollTop).toBe(1200);

    act(() => resize());
    viewport.scrollTop = 100;
    act(() => scroll.handleScroll(viewport));
    const reads = height.mock.calls.length;
    flushFrame();
    expect(height).toHaveBeenCalledTimes(reads);
    expect(viewport.scrollTop).toBe(100);

    act(() => resize());
    const previousFrame = [...frames.keys()][0];
    view.rerender(<View sessionId="second" blocks={[]} />);
    expect(frames.has(previousFrame)).toBe(false);
    expect(frames.size).toBe(1);
    flushFrame();
    expect(viewport.scrollTop).toBe(1200);
    act(() => resize());
    view.unmount();
    expect(frames.size).toBe(0);
  });
});
