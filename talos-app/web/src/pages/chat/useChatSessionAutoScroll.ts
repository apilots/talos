import { useEffect, useLayoutEffect, useRef } from "react";
import type { ChatRenderBlock } from "@/shared/types/ui";

const USER_MESSAGE_TOP_OFFSET_PX = 20;

export function useChatSessionAutoScroll({
  blocks,
  isStreaming,
  sessionId,
  streamError,
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
  const anchoredUserBlockIdRef = useRef<string | undefined>(undefined);
  const userAnchorPendingRef = useRef(false);
  const ignoreScrollRef = useRef(false);
  const ignoreScrollFrameRef = useRef<number | undefined>(undefined);
  const currentSessionIdRef = useRef(sessionId);

  function scrollToBottom() {
    const viewport = viewportRef.current;
    if (!viewport) {
      return;
    }
    viewport.scrollTop = viewport.scrollHeight;
    endRef.current?.scrollIntoView({ block: "end" });
  }

  function handleScroll(viewport: HTMLDivElement) {
    if (ignoreScrollRef.current) {
      return;
    }
    if (userAnchorPendingRef.current) {
      return;
    }
    const anchoredUserBlockId = anchoredUserBlockIdRef.current;
    const content = contentRef.current;
    const anchoredUserBlock = anchoredUserBlockId && content
      ? findUserBlock(content, anchoredUserBlockId)
      : undefined;
    if (anchoredUserBlock) {
      const currentOffset =
        anchoredUserBlock.getBoundingClientRect().top -
        viewport.getBoundingClientRect().top;
      if (Math.abs(currentOffset - USER_MESSAGE_TOP_OFFSET_PX) < 2) {
        return;
      }
    }
    const distanceToBottom =
      viewport.scrollHeight - viewport.scrollTop - viewport.clientHeight;
    stickToBottomRef.current = distanceToBottom < 80;
  }

  function anchorUserBlock(blockId: string) {
    const viewport = viewportRef.current;
    const content = contentRef.current;
    if (!viewport || !content) {
      return;
    }
    const target = findUserBlock(content, blockId);
    if (!target) {
      return;
    }
    const offset =
      target.getBoundingClientRect().top -
      viewport.getBoundingClientRect().top -
      USER_MESSAGE_TOP_OFFSET_PX;
    ignoreScrollRef.current = true;
    viewport.scrollTop += offset;
    const currentOffset =
      target.getBoundingClientRect().top - viewport.getBoundingClientRect().top;
    userAnchorPendingRef.current =
      Math.abs(currentOffset - USER_MESSAGE_TOP_OFFSET_PX) >= 2;
    if (ignoreScrollFrameRef.current !== undefined) {
      window.cancelAnimationFrame(ignoreScrollFrameRef.current);
    }
    ignoreScrollFrameRef.current = window.requestAnimationFrame(() => {
      ignoreScrollRef.current = false;
      ignoreScrollFrameRef.current = undefined;
    });
  }

  useEffect(() => {
    return () => {
      if (ignoreScrollFrameRef.current !== undefined) {
        window.cancelAnimationFrame(ignoreScrollFrameRef.current);
      }
    };
  }, []);

  useEffect(() => {
    const content = contentRef.current;
    if (!content || typeof ResizeObserver === "undefined") {
      return;
    }
    const observer = new ResizeObserver(() => {
      if (stickToBottomRef.current) {
        scrollToBottom();
      } else if (userAnchorPendingRef.current && anchoredUserBlockIdRef.current) {
        anchorUserBlock(anchoredUserBlockIdRef.current);
      }
    });
    observer.observe(content);
    return () => observer.disconnect();
  }, []);

  useLayoutEffect(() => {
    if (currentSessionIdRef.current !== sessionId) {
      currentSessionIdRef.current = sessionId;
      anchoredUserBlockIdRef.current = undefined;
      userAnchorPendingRef.current = false;
      stickToBottomRef.current = true;
    }
    const currentUserBlock = isStreaming ? [...blocks]
      .reverse()
      .find((block) => block.kind === "user") : undefined;
    if (currentUserBlock && anchoredUserBlockIdRef.current !== currentUserBlock.id) {
      anchoredUserBlockIdRef.current = currentUserBlock.id;
      userAnchorPendingRef.current = true;
      stickToBottomRef.current = false;
      anchorUserBlock(currentUserBlock.id);
      return;
    }
    if (currentUserBlock && userAnchorPendingRef.current) {
      anchorUserBlock(currentUserBlock.id);
      return;
    }
    if (!isStreaming) {
      userAnchorPendingRef.current = false;
    }
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
  }, [blocks, isStreaming, streamError, sessionId]);

  return {
    contentRef,
    endRef,
    handleScroll,
    viewportRef,
  };
}

function findUserBlock(content: HTMLDivElement, blockId: string) {
  return Array.from(
    content.querySelectorAll<HTMLElement>("[data-chat-user-block-id]"),
  ).find((element) => element.dataset.chatUserBlockId === blockId);
}
