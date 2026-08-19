import type { MessageQueue } from "@talos/sdk-browser";

interface MessageQueueSyncOptions {
  list: () => Promise<MessageQueue>;
  onChange: (queue: MessageQueue | undefined) => void;
  onError?: (error: unknown) => void;
}

export class MessageQueueSync {
  private highestObservedRevision = 0;
  private epoch = 0;
  private queue: MessageQueue | undefined;
  private refreshRequested = false;
  private refreshPromise: Promise<void> | undefined;

  constructor(private readonly options: MessageQueueSyncOptions) {}

  observe(revision: number) {
    if (revision <= this.highestObservedRevision) {
      return;
    }
    this.highestObservedRevision = revision;
    this.requestRefresh();
  }

  apply(queue: MessageQueue) {
    if (!this.queue || queue.revision >= this.queue.revision) {
      this.queue = queue;
      this.options.onChange(queue);
    }
    if (queue.revision < this.highestObservedRevision) {
      this.requestRefresh();
    }
  }

  clear() {
    this.epoch += 1;
    this.highestObservedRevision = 0;
    this.queue = undefined;
    this.refreshRequested = false;
    this.options.onChange(undefined);
  }

  refresh(): Promise<void> {
    this.refreshRequested = true;
    if (this.refreshPromise) {
      return this.refreshPromise;
    }
    this.refreshPromise = this.runRefreshLoop().finally(() => {
      this.refreshPromise = undefined;
    });
    return this.refreshPromise;
  }

  private async runRefreshLoop() {
    while (this.refreshRequested) {
      this.refreshRequested = false;
      const epoch = this.epoch;
      const queue = await this.options.list();
      if (epoch === this.epoch) {
        this.apply(queue);
      }
    }
  }

  private requestRefresh() {
    void this.refresh().catch((error: unknown) => this.options.onError?.(error));
  }
}
