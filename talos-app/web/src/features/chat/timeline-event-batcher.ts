import type { TimelineEvent } from "@talos/sdk-core";

const MAX_BATCH_EVENTS = 64;
const FRAME_DELAY_MS = 16;

/** Bound burst work; an applied cursor always belongs to a committed batch. */
export function createTimelineEventBatcher(
  apply: (events: TimelineEvent[]) => number,
  onError: (error: unknown) => void,
) {
  let queue: TimelineEvent[] = [];
  let timer: ReturnType<typeof setTimeout> | undefined;
  let disposed = false;

  const schedule = () => {
    if (!disposed && timer === undefined) timer = setTimeout(flush, FRAME_DELAY_MS);
  };
  const flush = () => {
    timer = undefined;
    if (disposed) return;
    if (queue.length) {
      try {
        const consumed = apply(queue.slice(0, MAX_BATCH_EVENTS));
        queue.splice(0, consumed);
      } catch (error) {
        cancel();
        onError(error);
        return;
      }
    }
    if (queue.length) schedule();
  };
  const cancel = () => {
    disposed = true;
    if (timer !== undefined) clearTimeout(timer);
    timer = undefined;
    queue = [];
  };
  return {
    push(event: TimelineEvent) {
      if (disposed) return;
      queue.push(event);
      if (timer === undefined) {
        // Deliver the first event promptly; coalesce the following burst.
        flush();
        schedule();
      }
    },
    cancel,
  };
}
