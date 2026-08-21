import { useEffect, useMemo, useState } from "react";
import type { SkillSuggestion } from "@talos/sdk-browser";

import { suggestSkills } from "../../api";
import { activeComposerTrigger, type ComposerTrigger } from "./composer-trigger";
import {
  filterComposerCommands,
  type ComposerCommandId,
  type ComposerSuggestion,
} from "./composer-suggestions";

interface UseComposerSuggestionsOptions {
  commands: readonly ComposerCommandId[];
  cursorIndex: number;
  enabled: boolean;
  isComposing: boolean;
  value: string;
}

export function useComposerSuggestions({
  commands,
  cursorIndex,
  enabled,
  isComposing,
  value,
}: UseComposerSuggestionsOptions) {
  const [skills, setSkills] = useState<SkillSuggestion[]>([]);
  const [error, setError] = useState<string>();
  const [resolvedTrigger, setResolvedTrigger] = useState<ComposerTrigger | null>(null);
  const [selectedIndex, setSelectedIndex] = useState(0);
  const [cancelledKey, setCancelledKey] = useState<string>();
  const trigger = useMemo(
    () => enabled && !isComposing ? activeComposerTrigger(value, cursorIndex) : null,
    [cursorIndex, enabled, isComposing, value],
  );
  const triggerKey = composerTriggerKey(trigger);
  const open = Boolean(trigger && triggerKey !== cancelledKey);
  const loading = Boolean(open && trigger !== resolvedTrigger);
  const currentSkills = trigger === resolvedTrigger ? skills : [];
  const currentError = trigger === resolvedTrigger ? error : undefined;

  useEffect(() => {
    if (!open || !trigger) {
      return;
    }

    let cancelled = false;
    const timer = window.setTimeout(() => {
      void suggestSkills(trigger.query)
        .then((data) => {
          if (!cancelled) {
            setSkills(data);
            setError(undefined);
            setResolvedTrigger(trigger);
          }
        })
        .catch((requestError: unknown) => {
          if (!cancelled) {
            setSkills([]);
            setError(requestError instanceof Error ? requestError.message : "Unable to load Skills");
            setResolvedTrigger(trigger);
          }
        });
    }, 120);

    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [open, trigger]);

  const suggestions = useMemo<ComposerSuggestion[]>(() => {
    if (!open || !trigger) {
      return [];
    }
    const commandSuggestions = trigger.kind === "slash"
      ? filterComposerCommands(commands, trigger.query)
      : [];
    return [
      ...commandSuggestions,
      ...currentSkills.map((skill): ComposerSuggestion => ({ type: "skill", skill })),
    ];
  }, [commands, currentSkills, open, trigger]);

  useEffect(() => {
    setSelectedIndex(0);
  }, [triggerKey]);

  useEffect(() => {
    if (cancelledKey && triggerKey !== cancelledKey) {
      setCancelledKey(undefined);
    }
  }, [cancelledKey, triggerKey]);

  const cancelCurrentTrigger = () => {
    setCancelledKey(triggerKey);
    setSelectedIndex(0);
  };

  const updateTriggerSession = (nextValue: string, nextCursorIndex: number) => {
    const nextTriggerKey = composerTriggerKey(
      activeComposerTrigger(nextValue, nextCursorIndex),
    );
    setCancelledKey((currentKey) =>
      currentKey && nextTriggerKey !== currentKey ? undefined : currentKey,
    );
  };

  return {
    cancelCurrentTrigger,
    error: currentError,
    loading,
    open,
    selectedIndex,
    setSelectedIndex,
    suggestions,
    trigger,
    updateTriggerSession,
  };
}

function composerTriggerKey(trigger: ReturnType<typeof activeComposerTrigger>) {
  return trigger ? `${trigger.kind}:${trigger.start}:${trigger.end}:${trigger.query}` : undefined;
}
