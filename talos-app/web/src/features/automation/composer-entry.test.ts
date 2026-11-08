import { afterEach, describe, expect, it, vi } from "vitest";
import { setTalosClientFactory, resetTalosClientFactory, type TalosWebClient } from "@/shared/api/client";
import { prepareAutomationMessage } from "./composer-entry";
import { apiErrorMessage, userVisibleError } from "@/shared/i18n/apiErrorMessage";
import { i18n } from "@/shared/i18n/setup";

afterEach(resetTalosClientFactory);
afterEach(async () => { await i18n.changeLanguage("en"); });
function client() {
  const list = vi.fn().mockResolvedValue({ data: [
    { name: "goal", scope: "project", path: "/project/goal/SKILL.md", enabled: true, read_only: false },
    { name: "goal", scope: "system", path: "/actual-worker/system/goal/SKILL.md", enabled: true, read_only: true },
    { name: "loop", scope: "system", path: "/actual-worker/system/loop/SKILL.md", enabled: true, read_only: true },
  ] });
  const resolveCommand = vi.fn().mockResolvedValue({ command: null });
  setTalosClientFactory(() => ({ skills: { list }, automation: { resolveCommand } }) as unknown as TalosWebClient);
  return { list, resolveCommand };
}
describe("explicit composer automation entry", () => {
  it("resolves Add and slash on the actual Worker and does not choose a same-name project Skill", async () => {
    const mocks = client();
    const fromAdd = await prepareAutomationMessage("Fix CI", "goal", "remote-node");
    mocks.resolveCommand.mockResolvedValue({ command: { type: "activate", mode: "goal", prompt: "Fix CI" } });
    const fromSlash = await prepareAutomationMessage("/goal Fix CI", undefined, "remote-node");
    expect(fromAdd).toEqual(fromSlash);
    expect(fromAdd).toEqual({ kind: "message", prompt: "Fix CI", skillSelection: { name: "goal", path: "/actual-worker/system/goal/SKILL.md" } });
    expect(mocks.list).toHaveBeenCalledWith({ nodeId: "remote-node", include_disabled: false });
    expect(mocks.resolveCommand).toHaveBeenCalledWith({ text: "/goal Fix CI" });
  });
  it("keeps the shared slash prompt's cadence and original tail equal to Add without another parser", async () => {
    const mocks = client();
    const prompt = "5m  Check deployment\n  then report changes";
    mocks.resolveCommand.mockResolvedValue({ command: { type: "activate", mode: "loop", prompt } });
    const fromSlash = await prepareAutomationMessage("/loop " + prompt, undefined, "remote-node");
    const fromAdd = await prepareAutomationMessage(prompt, "loop", "remote-node");
    expect(fromSlash).toEqual(fromAdd);
    expect(fromSlash).toMatchObject({ kind: "message", prompt, skillSelection: { name: "loop" } });
    expect(mocks.resolveCommand).toHaveBeenCalledTimes(1);
    expect(mocks.resolveCommand).toHaveBeenCalledWith({ text: "/loop " + prompt });
  });
  it.each(["goal", "loop"] as const)("preserves a sleeping Talo’s /%s command for Server admission", async (mode) => {
    const mocks = client();
    const prompt = "Check  the deployment";
    const text = `/${mode} ${prompt}`;
    mocks.resolveCommand.mockResolvedValue({ command: { type: "activate", mode, prompt } });
    expect(await prepareAutomationMessage(text, undefined, "talo", true)).toEqual({
      kind: "message", prompt: text, skillSelection: undefined,
    });
    expect(mocks.list).not.toHaveBeenCalled();
  });
  it("keeps ordinary conversation untouched and does not discover automation from vague tasks", async () => {
    const mocks = client();
    expect(await prepareAutomationMessage("Keep working until tests pass", undefined, "node")).toEqual({ kind: "message", prompt: "Keep working until tests pass", skillSelection: undefined });
    expect(mocks.list).not.toHaveBeenCalled();
    expect(mocks.resolveCommand).not.toHaveBeenCalled();
  });
  it("uses shared command results and rejects conflicting explicit modes", async () => {
    const mocks = client();
    mocks.resolveCommand.mockResolvedValue({ command: { type: "activate", mode: "loop", prompt: "5m Check" } });
    await expect(prepareAutomationMessage("/loop 5m Check", "goal", "node")).rejects.toThrow("conflicts");
    mocks.resolveCommand.mockResolvedValue({ command: { type: "read", mode: "loop" } });
    expect(await prepareAutomationMessage("/loop", undefined, "node")).toEqual({ kind: "management", command: { type: "read", mode: "loop" } });
    expect(mocks.list).not.toHaveBeenCalled();
  });
  it("fails when a real protected Skill is absent without inventing a path", async () => {
    const mocks = client(); mocks.list.mockResolvedValue({ data: [] });
    await expect(prepareAutomationMessage("Check CI", "loop", "node")).rejects.toThrow("unavailable");
  });
  it("renders a retained selection conflict in the current language", async () => {
    const mocks = client();
    mocks.resolveCommand.mockResolvedValue({ command: { type: "activate", mode: "loop", prompt: "Check CI" } });
    const failure: unknown = await prepareAutomationMessage("/loop Check CI", "goal", "node").catch((error: unknown) => error);
    const visible = userVisibleError(failure, "The message could not be sent.");
    await i18n.changeLanguage("zh-CN");
    expect(apiErrorMessage(visible)).toBe("命令与已选择的自动化模式冲突。");
    await i18n.changeLanguage("en");
    expect(apiErrorMessage(visible)).toBe("The command conflicts with the selected automation mode.");
  });
});
