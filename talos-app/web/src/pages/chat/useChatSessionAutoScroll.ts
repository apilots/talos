import { useEffect, useLayoutEffect, useRef } from "react";
import type { ChatRenderBlock } from "@/shared/types/ui";

export function useChatSessionAutoScroll({
  blocks,
  sessionId,
  streamError,
}: {
  blocks: ChatRenderBlock[];
  sessionId: string;
  streamError?: string;
}) {
  const viewportRef = useRef<HTMLDivElement | null>(null);
  const contentRef = useRef<HTMLDivElement | null>(null);
  const endRef = useRef<HTMLDivElement | null>(null);
  const stickToBottomRef = useRef(true);

  function scrollToBottom() {
    const viewport = viewportRef.current;
    if (!viewport) {
      return;
    }
    viewport.scrollTop = viewport.scrollHeight;
    endRef.current?.scrollIntoView({ block: "end" });
  }

  function handleScroll(viewport: HTMLDivElement) {
    const distanceToBottom =
      viewport.scrollHeight - viewport.scrollTop - viewport.clientHeight;
    stickToBottomRef.current = distanceToBottom < 80;
  }

  useEffect(() => {
    stickToBottomRef.current = true;
  }, [sessionId]);

  useEffect(() => {
    const content = contentRef.current;
    if (!content || typeof ResizeObserver === "undefined") {
      return;
    }
    const observer = new ResizeObserver(() => {
      if (stickToBottomRef.current) {
        scrollToBottom();
      }
    });
    observer.observe(content);
    return () => observer.disconnect();
  }, []);

  useLayoutEffect(() => {
    if (!viewportRef.current || !stickToBottomRef.current) {
      return;
    }
    scrollToBottom();
    const frameId = window.requestAnimationFrame(scrollToBottom);
    const timeoutId = window.setTimeout(scrollToBottom, 120);
    return () => {
      window.cancelAnimationFrame(frameId);
      window.clearTimeout(timeoutId);
    };
  }, [blocks, streamError, sessionId]);

  return {
    contentRef,
    endRef,
    handleScroll,
    scrollToBottom,
    stickToBottomRef,
    viewportRef,
  };
}
