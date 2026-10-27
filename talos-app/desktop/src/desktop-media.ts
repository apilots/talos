import type { WebContents } from "electron";
import { isDesktopRenderer } from "./desktop-window";
import type { AppProtocol } from "@talos/sdk-core";
import { TALOS_IPC } from "./ipc";

type PendingFrame = {
  streamId: string;
  owner: WebContents;
  resolve: () => void;
  reject: (error: Error) => void;
  timer: ReturnType<typeof setTimeout>;
};
type OwnerWatch = {
  destroyed: () => void;
  navigating: (event: Electron.Event<Electron.WebContentsDidStartNavigationEventParams>) => void;
};

/** Route only to the product renderer that opened this immutable stream. */
class DesktopMediaHost {
  private readonly owners = new Map<string, WebContents>();
  private readonly pending = new Map<string, PendingFrame>();
  private readonly watches = new Map<WebContents, OwnerWatch>();
  private readonly closeBackend = new Map<string, () => void>();

  own(streamId: string, contents: WebContents, closeBackend?: () => void): void {
    if (!isDesktopRenderer(contents)
      || this.owners.has(streamId) || this.owners.size >= 2) {
      throw new Error("Invalid Desktop media owner.");
    }
    this.owners.set(streamId, contents);
    if (closeBackend) this.closeBackend.set(streamId, closeBackend);
    if (!this.watches.has(contents)) {
      const destroyed = () => this.releaseOwner(contents);
      const navigating: OwnerWatch["navigating"] = (event) => {
        if (event.isMainFrame && !event.isSameDocument) this.releaseOwner(contents);
      };
      this.watches.set(contents, { destroyed, navigating });
      contents.once("destroyed", destroyed);
      contents.on("render-process-gone", destroyed);
      contents.on("did-start-navigation", navigating);
    }
  }

  release(streamId: string, contents: WebContents, notifyBackend = true): void {
    if (this.owners.get(streamId) !== contents) return;
    this.owners.delete(streamId);
    const stop = this.closeBackend.get(streamId);
    this.closeBackend.delete(streamId);
    if (notifyBackend) stop?.();
    for (const [id, pending] of this.pending) {
      if (pending.streamId !== streamId) continue;
      clearTimeout(pending.timer);
      this.pending.delete(id);
      pending.reject(new Error("Desktop media stream closed."));
    }
    if (![...this.owners.values()].includes(contents)) {
      const watch = this.watches.get(contents);
      if (watch) {
        contents.removeListener("destroyed", watch.destroyed);
        contents.removeListener("render-process-gone", watch.destroyed);
        contents.removeListener("did-start-navigation", watch.navigating);
        this.watches.delete(contents);
      }
    }
  }

  private releaseOwner(contents: WebContents): void {
    for (const [stream, owner] of this.owners) {
      if (owner === contents) this.release(stream, contents);
    }
  }

  async frame(requestId: string, frame: AppProtocol.DesktopMediaFrame): Promise<{ kind: "done" }> {
    const owner = this.owners.get(frame.streamId);
    if (!owner) throw new Error("Desktop viewer is unavailable.");
    try {
      if (owner.isDestroyed() || this.pending.size >= 4
        || frame.dataBase64 && frame.dataBase64.length > 88 * 1024) {
        throw new Error("Desktop viewer is unavailable.");
      }
      await new Promise<void>((resolve, reject) => {
        const timer = setTimeout(() => {
          this.pending.delete(requestId);
          reject(new Error("Desktop viewer stopped consuming media."));
        }, 1800);
        this.pending.set(requestId, { streamId: frame.streamId, owner, resolve, reject, timer });
        owner.send(TALOS_IPC.desktopFrame, { ...frame, requestId });
      });
      if (frame.dataBase64 === null) this.release(frame.streamId, owner, false);
      return { kind: "done" };
    } catch (error) {
      this.release(frame.streamId, owner);
      throw error;
    }
  }

  acknowledge(requestId: string, contents: WebContents): void {
    const pending = this.pending.get(requestId);
    if (!pending || pending.owner !== contents) throw new Error("Invalid Desktop media acknowledgement.");
    clearTimeout(pending.timer);
    this.pending.delete(requestId);
    pending.resolve();
  }

  cancel(requestId: string): void {
    const pending = this.pending.get(requestId);
    if (!pending) return;
    this.release(pending.streamId, pending.owner);
  }

  reset(): void {
    // The app-server generation itself is closing; it owns transport teardown.
    for (const [streamId, owner] of this.owners) this.release(streamId, owner, false);
  }
}
export const desktopMediaHost = new DesktopMediaHost();
