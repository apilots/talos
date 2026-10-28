import { expect, it, vi } from "vitest";
const native = vi.hoisted(() => ({ open: vi.fn().mockResolvedValue(undefined), openApp: vi.fn().mockResolvedValue(undefined) }));
vi.mock("electron", () => ({ app: { getFileIcon: async () => ({ toDataURL: () => "icon" }) }, shell: { openExternal: vi.fn(), openPath: vi.fn() } }));

vi.mock("which", () => ({ default: async (command: string) => ["google-chrome", "gnome-terminal"].includes(command) ? `/usr/bin/${command}` : null }));
vi.mock("./preview-application-tools", () => ({ previewApplicationTools: async () => ({
  appPath: async (id: string) => { if (id === "com.google.Chrome" || id === "com.apple.Terminal") return `/Applications/${id}.app`; throw new Error("Not installed"); }, open: native.open, openApp: native.openApp,
}) }));
import { openPreviewApplication, previewApplications } from "./preview-applications";
it("lists installed targets and opens terminal directories without executing the preview file", async () => {
  const targets = await previewApplications("file");
  expect(targets.map(target => target.id)).toEqual(["system", "chrome", "terminal"]);
  await openPreviewApplication(targets, "terminal", { kind: "file", path: "/tmp/preview/report.sh", directory: "/tmp/preview" });
  const call = process.platform === "darwin" ? native.open.mock.calls[0] : native.openApp.mock.calls[0];
  expect(JSON.stringify(call)).toContain("/tmp/preview");
  expect(JSON.stringify(call)).not.toContain("report.sh");
  expect((await previewApplications("webpage")).map(target => target.id)).toEqual(["system", "chrome"]);
});
