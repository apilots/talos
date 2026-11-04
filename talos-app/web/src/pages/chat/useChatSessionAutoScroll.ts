import { useEffect, useLayoutEffect, useRef } from "react";
import type { ChatRenderBlock } from "@/shared/types/ui";

/** Follow new execution output unless the reader deliberately scrolls away. */
export function useChatSessionAutoScroll({
  blocks, isStreaming, sessionId, streamError,
}: {
  blocks: ChatRenderBlock[];
  isStreaming: boolean;
  sessionId: string;
  streamError?: string;
}) {
  const viewportRef = useRef<HTMLDivElement | null>(null);
  const contentRef = useRef<HTMLDivElement | null>(null);
  const endRef = useRef<HTMLDivElement | null>(null);
  const stickToBottomRef = useRef(true);
  const wasStreamingRef = useRef(false);
  const currentSessionIdRef = useRef(sessionId);
  const scrollFrameRef = useRef<number | null>(null);

  function scheduleScrollToBottom() {
    if (scrollFrameRef.current !== null) {
      return;
    }
    scrollFrameRef.current = window.requestAnimationFrame(() => {
      scrollFrameRef.current = null;
      const viewport = viewportRef.current;
      if (viewport && stickToBottomRef.current) {
        viewport.scrollTop = viewport.scrollHeight;
      }
    });
  }

  function handleScroll(viewport: HTMLDivElement) {
    stickToBottomRef.current =
      viewport.scrollHeight - viewport.scrollTop - viewport.clientHeight < 80;
  }

  useEffect(() => {
    const content = contentRef.current;
    if (!content || typeof ResizeObserver === "undefined") {
      return;
    }
    const observer = new ResizeObserver(scheduleScrollToBottom);
    observer.observe(content);
    return () => observer.disconnect();
  }, []);

  useLayoutEffect(() => {
    if (currentSessionIdRef.current !== sessionId || (isStreaming && !wasStreamingRef.current)) {
      stickToBottomRef.current = true;
    }
    currentSessionIdRef.current = sessionId;
    wasStreamingRef.current = isStreaming;
    scheduleScrollToBottom();
    return () => {
      if (scrollFrameRef.current !== null) {
        window.cancelAnimationFrame(scrollFrameRef.current);
        scrollFrameRef.current = null;
      }
    };
  }, [blocks, isStreaming, streamError, sessionId]);

  return { contentRef, endRef, handleScroll, viewportRef };
}
