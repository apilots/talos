export type ComposerTriggerKind = "slash" | "skill";

export interface ComposerTrigger {
  kind: ComposerTriggerKind;
  start: number;
  end: number;
  query: string;
}

const QUERY_CHARACTER = /^[A-Za-z0-9_.-]$/;
const TRIGGER_BOUNDARY = /(^|[\s([{"'，。！？：；])/;

/** Finds the active command or Skill query immediately before the cursor. */
export function activeComposerTrigger(value: string, cursorIndex: number): ComposerTrigger | null {
  if (cursorIndex < 0 || cursorIndex > value.length || QUERY_CHARACTER.test(value.charAt(cursorIndex))) {
    return null;
  }

  const prefix = value.slice(0, cursorIndex);
  const match = /(^|[\s([{"'，。！？：；])([/$])([A-Za-z0-9_.-]*)$/.exec(prefix);
  if (!match || match.index === undefined || !TRIGGER_BOUNDARY.test(match[1] ?? "")) {
    return null;
  }

  const boundary = match[1] ?? "";
  if (match[2] === "/" && boundary !== "" && !/\s/.test(boundary)) {
    return null;
  }
  return {
    kind: match[2] === "/" ? "slash" : "skill",
    start: match.index + boundary.length,
    end: cursorIndex,
    query: match[3] ?? "",
  };
}

/** Replaces only the active query and preserves the surrounding draft. */
export function replaceComposerTrigger(
  value: string,
  trigger: ComposerTrigger,
  replacement: string,
): { value: string; cursorIndex: number } {
  const nextValue = `${value.slice(0, trigger.start)}${replacement}${value.slice(trigger.end)}`;
  return {
    value: nextValue,
    cursorIndex: trigger.start + replacement.length,
  };
}

/** Replaces a Skill query with its plain-text mention and a safe trailing delimiter. */
export function replaceComposerTriggerWithSkill(
  value: string,
  trigger: ComposerTrigger,
  skillName: string,
): { value: string; cursorIndex: number } {
  const suffix = trigger.end === value.length ? " " : "";
  return replaceComposerTrigger(value, trigger, `$${skillName}${suffix}`);
}
