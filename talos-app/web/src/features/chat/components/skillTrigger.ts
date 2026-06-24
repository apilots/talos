export interface SkillTrigger {
  start: number;
  end: number;
  query: string;
}

const SKILL_QUERY_RE = /^[A-Za-z0-9_.-]*$/;

export function activeSkillTrigger(value: string, cursorIndex: number): SkillTrigger | null {
  if (cursorIndex < 0 || cursorIndex > value.length) {
    return null;
  }
  const nextChar = value.charAt(cursorIndex);
  if (isSkillQueryChar(nextChar)) {
    return null;
  }
  const prefix = value.slice(0, cursorIndex);
  const match = /(^|\s)\$([A-Za-z0-9_.-]*)$/.exec(prefix);
  if (!match || match.index === undefined) {
    return null;
  }
  const query = match[2] ?? "";
  if (!SKILL_QUERY_RE.test(query)) {
    return null;
  }
  return {
    start: match.index + match[1].length,
    end: cursorIndex,
    query,
  };
}

export function replaceSkillTrigger(
  value: string,
  trigger: SkillTrigger,
  skillName: string,
): { value: string; cursorIndex: number } {
  const before = value.slice(0, trigger.start);
  const after = value.slice(trigger.end);
  const prefix = before.length > 0 && !/\s$/.test(before) ? `${before} ` : before;
  const token = `$${skillName}`;
  const suffix = after.length === 0 || /^\s/.test(after) ? after || " " : ` ${after}`;
  const nextValue = `${prefix}${token}${suffix}`;
  const cursorIndex = prefix.length + token.length + (suffix.length > 0 && /^\s/.test(suffix) ? 1 : 0);
  return { value: nextValue, cursorIndex };
}

function isSkillQueryChar(value: string) {
  return /^[A-Za-z0-9_.-]$/.test(value);
}
