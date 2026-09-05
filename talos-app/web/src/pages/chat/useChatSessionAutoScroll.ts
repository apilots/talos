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

  function scrollToBottom() {
    const viewport = viewportRef.current;
    if (viewport && stickToBottomRef.current) {
      viewport.scrollTop = viewport.scrollHeight;
    }
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
    const observer = new ResizeObserver(scrollToBottom);
    observer.observe(content);
    return () => observer.disconnect();
  }, []);

  useLayoutEffect(() => {
    if (currentSessionIdRef.current !== sessionId || (isStreaming && !wasStreamingRef.current)) {
      stickToBottomRef.current = true;
    }
    currentSessionIdRef.current = sessionId;
    wasStreamingRef.current = isStreaming;
    scrollToBottom();
    const frameId = window.requestAnimationFrame(scrollToBottom);
    return () => window.cancelAnimationFrame(frameId);
  }, [blocks, isStreaming, streamError, sessionId]);

  return { contentRef, endRef, handleScroll, viewportRef };
}
