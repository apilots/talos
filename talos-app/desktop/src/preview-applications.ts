import { app, shell } from "electron";
import { previewApplicationTools } from "./preview-application-tools";
import which from "which";
import type { PreviewOpenTarget, PreviewOpenTargetId } from "@talos/sdk-app-server/desktop";

const candidates = [
  { id: "chrome", label: "Google Chrome", mac: "com.google.Chrome", commands: ["google-chrome", "chrome", "chrome.exe"], kind: "browser" },
  { id: "safari", label: "Safari", mac: "com.apple.Safari", commands: [], kind: "browser" },
  { id: "firefox", label: "Firefox", mac: "org.mozilla.firefox", commands: ["firefox", "firefox.exe"], kind: "browser" },
  { id: "edge", label: "Microsoft Edge", mac: "com.microsoft.edgemac", commands: ["microsoft-edge", "msedge.exe"], kind: "browser" },
  { id: "vscode", label: "Visual Studio Code", mac: "com.microsoft.VSCode", commands: ["code", "code.cmd"], kind: "editor" },
  { id: "iterm", label: "iTerm", mac: "com.googlecode.iterm2", commands: [], kind: "terminal" },
  { id: "terminal", label: "Terminal", mac: "com.apple.Terminal", commands: ["gnome-terminal", "wt.exe"], kind: "terminal" },
] as const;
type InstalledTarget = PreviewOpenTarget & { program?: string; kind: "system" | "browser" | "editor" | "terminal" };

/** Native app discovery/launch only. Does not resolve or read resource sources. */
export async function previewApplications(kind: "webpage" | "file"): Promise<InstalledTarget[]> {
  const { appPath } = await previewApplicationTools();
  const installed: InstalledTarget[] = [{ id: "system", label: kind === "webpage" ? "System default browser" : "System default application", kind: "system" }];
  await Promise.all(candidates.filter(candidate => kind === "file" || candidate.kind === "browser").map(async candidate => {
    let program: string | undefined;
    if (process.platform === "darwin") {
      try { program = await appPath(candidate.mac); } catch { return; }
    } else {
      for (const command of candidate.commands) { const found = await which(command, { nothrow: true }); if (found) { program = found; break; } }
    }
    if (!program) return;
    let icon: string | undefined;
    try { icon = (await app.getFileIcon(program, { size: "small" })).toDataURL(); } catch { /* Optional OS artwork does not determine availability. */ }
    installed.push({ id: candidate.id, label: candidate.label, kind: candidate.kind, program, icon });
  }));
  // Discovery completion order must not change the visible target order.
  return installed.sort((a, b) => (a.id === "system" ? -1 : candidates.findIndex(c => c.id === a.id)) - (b.id === "system" ? -1 : candidates.findIndex(c => c.id === b.id)));
}

export async function openPreviewApplication(targets: InstalledTarget[], targetId: PreviewOpenTargetId, resource: { kind: "webpage"; url: string } | { kind: "file"; path: string; directory: string }): Promise<void> {
  const { open, openApp } = await previewApplicationTools();
  const target = targets.find(target => target.id === targetId);
  if (!target) throw new Error("The selected application is unavailable.");
  if (target.kind === "terminal") {
    if (resource.kind !== "file" || !target.program) throw new Error("A file directory is required.");
    if (process.platform === "darwin") await open(resource.directory, { app: { name: target.program } });
    else await openApp(target.program, { arguments: [target.program.toLowerCase().endsWith("wt.exe") ? "-d" : "--working-directory", resource.directory] });
    return;
  }
  const destination = resource.kind === "webpage" ? resource.url : resource.path;
  if (target.id === "system") {
    if (resource.kind === "webpage") await shell.openExternal(destination);
    else { const error = await shell.openPath(destination); if (error) throw new Error("The system application could not open this file."); }
  } else await open(destination, { app: { name: target.program! } });
}
