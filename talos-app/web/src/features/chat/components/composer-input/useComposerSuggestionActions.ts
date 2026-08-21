import type { KeyboardEvent, RefObject } from "react";

import type { ChatComposerProps } from "../ChatComposer.types";
import {
  replaceComposerTrigger,
  replaceComposerTriggerWithSkill,
} from "./composer-trigger";
import type { ComposerSuggestion } from "./composer-suggestions";
import type { useComposerSuggestions } from "./useComposerSuggestions";

interface ComposerSuggestionActionsOptions {
  draft: string;
  modelTriggerRef: RefObject<HTMLButtonElement | null>;
  moveCursorToPosition: (cursorIndex: number) => void;
  onCommand?: ChatComposerProps["onCommand"];
  onDraftChange: (value: string) => void;
  onResetHistory: () => void;
  session: ReturnType<typeof useComposerSuggestions>;
}

/** Owns the distinct inline-insertion, direct-command, and cancellation interaction paths. */
export function useComposerSuggestionActions({
  draft,
  modelTriggerRef,
  moveCursorToPosition,
  onCommand,
  onDraftChange,
  onResetHistory,
  session,
}: ComposerSuggestionActionsOptions) {
  const selectSuggestion = (suggestion: ComposerSuggestion) => {
    const trigger = session.trigger;
    if (!trigger) {
      return;
    }

    onResetHistory();
    if (suggestion.type === "skill") {
      const next = replaceComposerTriggerWithSkill(draft, trigger, suggestion.skill.name);
      onDraftChange(next.value);
      moveCursorToPosition(next.cursorIndex);
      return;
    }

    const next = replaceComposerTrigger(draft, trigger, "");
    onDraftChange(next.value);
    if (suggestion.id === "model") {
      modelTriggerRef.current?.click();
      return;
    }

    moveCursorToPosition(next.cursorIndex);
    const execution = onCommand?.(suggestion.id);
    if (execution) {
      void execution.catch(() => undefined);
    }
  };

  const handleKeyDown = (event: KeyboardEvent<HTMLTextAreaElement>) => {
    if (!session.open || event.nativeEvent.isComposing) {
      return false;
    }

    const suggestionCount = session.suggestions.length;
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      if (suggestionCount > 0) {
        const offset = event.key === "ArrowDown" ? 1 : -1;
        session.setSelectedIndex((index) =>
          Math.max(0, Math.min(suggestionCount - 1, index + offset)),
        );
      }
      return true;
    }

    if (event.key === "Enter" || event.key === "Tab") {
      event.preventDefault();
      const suggestion = session.suggestions[session.selectedIndex];
      if (suggestion) {
        selectSuggestion(suggestion);
      }
      return true;
    }

    if (event.key === "Escape") {
      event.preventDefault();
      session.cancelCurrentTrigger();
      return true;
    }

    return false;
  };

  return { handleKeyDown, selectSuggestion };
}
