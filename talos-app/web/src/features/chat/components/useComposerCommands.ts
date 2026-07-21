import { useMemo, useRef, useState } from "react";
import type React from "react";

import {
  filterComposerCommands,
  type ComposerCommand,
} from "./composer-commands";

interface UseComposerCommandsParams {
  draft: string;
  enabled: boolean;
  onCommand?: (command: ComposerCommand) => Promise<void> | void;
  onDraftChange: (value: string) => void;
  onExecute: () => void;
  openModelPicker: () => void;
}

export function useComposerCommands({
  draft,
  enabled,
  onCommand,
  onDraftChange,
  onExecute,
  openModelPicker,
}: UseComposerCommandsParams) {
  const [selectedIndex, setSelectedIndex] = useState(0);
  const [dismissedDraft, setDismissedDraft] = useState<string>();
  const draftRef = useRef(draft);
  draftRef.current = draft;
  const commands = useMemo(
    () => enabled && dismissedDraft !== draft ? filterComposerCommands(draft) : [],
    [dismissedDraft, draft, enabled],
  );

  const reset = () => {
    setDismissedDraft(undefined);
    setSelectedIndex(0);
  };

  const execute = (command: ComposerCommand) => {
    onExecute();
    reset();
    if (command === "model") {
      onDraftChange("");
      openModelPicker();
      return;
    }
    const result = onCommand?.(command);
    if (result instanceof Promise) {
      return result
        .then(() => {
          if (draftRef.current === draft) {
            onDraftChange("");
          }
        })
        .catch(() => undefined);
    }
    onDraftChange("");
  };

  const handleKeyDown = (event: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (commands.length === 0) {
      return false;
    }
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      const offset = event.key === "ArrowDown" ? 1 : -1;
      setSelectedIndex((index) => Math.max(0, Math.min(commands.length - 1, index + offset)));
      return true;
    }
    if (event.key === "Enter") {
      event.preventDefault();
      void execute(commands[selectedIndex] ?? commands[0]);
      return true;
    }
    if (event.key === "Escape") {
      event.preventDefault();
      setDismissedDraft(draft);
      return true;
    }
    return false;
  };

  return { commands, execute, handleKeyDown, reset, selectedIndex, setSelectedIndex };
}
