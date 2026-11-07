import { notifyManager } from "@tanstack/react-query";
import { vi } from "vitest";

// Query notifications (TanStack's notifyManager) run on a timer. One queued as a test ends could fire after its file's
// jsdom is gone and React then reaches for `window` (§19a, WEB-U-195). The shared setup runs them through this counting
// scheduler and waits for none to be pending after each test.
// The bound's own timer is real time even when a test left fake timers on (review of f1480992).
const realSetTimeout = globalThis.setTimeout;
const realClearTimeout = globalThis.clearTimeout;
let pending = 0;
let delay = 0;
const idle = new Set<() => void>();

/** Routes every query notification through the counting scheduler. */
export function countNotifications() {
  notifyManager.setScheduler((callback) => {
    pending += 1;
    setTimeout(() => {
      try {
        callback();
      } finally {
        pending -= 1;
        if (pending === 0) for (const done of idle) done();
        if (pending === 0) idle.clear();
      }
    }, delay);
  });
}

/** Delays every query notification by `ms` (WEB-U-195 only: a notification forced past its test's end). */
export function delayNotifications(ms: number) {
  delay = ms;
}

/**
 * Resolves once no query notification is pending, or fails after `boundMs` of real time. A notification a test left on
 * fake timers is run here first, so the wait never hangs on a clock nobody advances.
 */
export function notificationsSettled(boundMs = 1000): Promise<void> {
  // This runs every pending fake timer of the test, not only query notifications; it comes after RTL's unmount, so
  // nothing of the test is still listening.
  if (pending > 0 && vi.isFakeTimers()) vi.runOnlyPendingTimers();
  if (pending === 0) return Promise.resolve();
  return new Promise((resolve, reject) => {
    const timer = realSetTimeout(() => {
      idle.delete(done);
      reject(
        new Error(
          `query notifications still pending ${boundMs} ms after the test`,
        ),
      );
    }, boundMs);
    const done = () => {
      realClearTimeout(timer);
      resolve();
    };
    idle.add(done);
  });
}
