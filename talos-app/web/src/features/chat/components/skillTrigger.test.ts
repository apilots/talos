import { describe, expect, it } from "vitest";

import { activeSkillTrigger, replaceSkillTrigger } from "./skillTrigger";

describe("skill trigger parsing", () => {
  it("matches a skill token at the start of the draft", () => {
    expect(activeSkillTrigger("$br", 3)).toEqual({ start: 0, end: 3, query: "br" });
  });

  it("matches a skill token after whitespace in body text", () => {
    expect(activeSkillTrigger("please use $br now", 14)).toEqual({
      start: 11,
      end: 14,
      query: "br",
    });
  });

  it("does not match a skill token joined to body text", () => {
    expect(activeSkillTrigger("please$br", 9)).toBeNull();
  });

  it("does not match when the cursor is inside the token", () => {
    expect(activeSkillTrigger("please $browser now", 10)).toBeNull();
  });

  it("replaces a body token with spaces around the selected skill", () => {
    const trigger = activeSkillTrigger("please use $br now", 14);
    expect(trigger).not.toBeNull();
    expect(replaceSkillTrigger("please use $br now", trigger!, "browser")).toEqual({
      value: "please use $browser now",
      cursorIndex: 20,
    });
  });
});
