import { useEffect, useMemo, useRef } from "react";

import type { SessionInputHistory } from "@/shared/types/ui";
import { buildInputHistoryNavigationItems } from "../composerInputHistory";

interface ComposerInputHistoryOptions {
  disabled?: boolean;
  inputHistory?: SessionInputHistory;
  moveCursorToPosition: (cursorIndex: number) => void;
  onDraftChange: (value: string) => void;
  value: string;
}

/** Owns keyboard traversal of committed inputs without replacing native text editing. */
export function useComposerInputHistory({
  disabled,
  inputHistory,
  moveCursorToPosition,
  onDraftChange,
  value,
}: ComposerInputHistoryOptions) {
  const historyIndexRef = useRef<number | null>(null);
  const historyDraftRef = useRef("");
  const items = useMemo(() => buildInputHistoryNavigationItems(inputHistory), [inputHistory]);
  const itemsKey = useMemo(() => items.map((item) => item.key).join("|"), [items]);

  const reset = () => {
    historyIndexRef.current = null;
    historyDraftRef.current = "";
  };

  useEffect(() => {
    historyIndexRef.current = null;
    historyDraftRef.current = "";
  }, [itemsKey]);

  const setDraftFromHistory = (nextValue: string) => {
    onDraftChange(nextValue);
    moveCursorToPosition(nextValue.length);
  };

  const canNavigate = (event: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (
      disabled ||
      event.altKey ||
      event.ctrlKey ||
      event.metaKey ||
      event.nativeEvent.isComposing ||
      items.length === 0
    ) {
      return false;
    }

    if (historyIndexRef.current !== null) {
      return true;
    }

    return event.currentTarget.value.length === 0 ||
      event.currentTarget.selectionStart === event.currentTarget.value.length;
  };

  const handleKeyDown = (event: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (!canNavigate(event)) {
      return false;
    }

    if (event.key === "ArrowUp") {
      event.preventDefault();
      const nextIndex = historyIndexRef.current === null
        ? items.length - 1
        : Math.max(0, historyIndexRef.current - 1);
      if (historyIndexRef.current === null) {
        historyDraftRef.current = value;
      }
      historyIndexRef.current = nextIndex;
      setDraftFromHistory(items[nextIndex]?.text ?? "");
      return true;
    }

    if (event.key === "ArrowDown" && historyIndexRef.current !== null) {
      event.preventDefault();
      const nextIndex = historyIndexRef.current + 1;
      if (nextIndex >= items.length) {
        const restoredDraft = historyDraftRef.current;
        reset();
        setDraftFromHistory(restoredDraft);
        return true;
      }

      historyIndexRef.current = nextIndex;
      setDraftFromHistory(items[nextIndex]?.text ?? "");
      return true;
    }

    return false;
  };

  return { handleKeyDown, reset };
}
