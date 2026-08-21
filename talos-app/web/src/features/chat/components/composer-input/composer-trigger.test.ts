import { describe, expect, it } from "vitest";

import {
  activeComposerTrigger,
  replaceComposerTrigger,
  replaceComposerTriggerWithSkill,
} from "./composer-trigger";

describe("activeComposerTrigger", () => {
  it.each([
    ["/", 1, "slash", ""],
    ["please /web", 11, "slash", "web"],
    ["请检查： /web", 9, "slash", "web"],
    ["please $web", 11, "skill", "web"],
  ])("finds a supported trigger in %s", (value, cursorIndex, kind, query) => {
    expect(activeComposerTrigger(value, cursorIndex)).toMatchObject({ kind, query });
  });

  it.each(["https://example.com", "src/components/editor.tsx", "1/2", "please/web", "请检查：/web", "text(/web"])(
    "does not trigger inside %s",
    (value) => expect(activeComposerTrigger(value, value.length)).toBeNull(),
  );

  it("replaces only the active query", () => {
    const value = "please /web now";
    const trigger = activeComposerTrigger(value, 11);
    expect(trigger).not.toBeNull();
    expect(replaceComposerTrigger(value, trigger!, "$browser")).toEqual({
      value: "please $browser now",
      cursorIndex: 15,
    });
  });

  it("delimits a Skill mention inserted at the end of the draft", () => {
    const trigger = activeComposerTrigger("use /bro", 8);

    expect(replaceComposerTriggerWithSkill("use /bro", trigger!, "browser")).toEqual({
      value: "use $browser ",
      cursorIndex: 13,
    });
  });

  it("preserves an existing delimiter after a body Skill query", () => {
    const value = "use /bro now";
    const trigger = activeComposerTrigger(value, 8);

    expect(replaceComposerTriggerWithSkill(value, trigger!, "browser")).toEqual({
      value: "use $browser now",
      cursorIndex: 12,
    });
  });
});
