import type { Page, TestInfo } from "@playwright/test";

// The leaf module, never the store: the store's imports reach the generated validators, which Playwright's loader
// can't resolve, and one such import stops the whole suite from loading.
import { PUBLISH_FALLBACK_MS } from "../src/transcript/publish-timing";

/**
 * A record of the page's event streams, for a failure's report (TEST-PLAN §19a, the "Sending…" defect): each
 * EventSource the page creates, opens, closes, and each error, and its first message, with the page's clock. It wraps
 * the browser's own EventSource from outside the app, so a page that never creates its stream can be told from one
 * whose stream never opened (created, never `open`: its request got no headers). Beside it, on the same clock:
 * - each held-messages request (`…/messages/queued`), to set the stream's creation against the page's first request;
 * - how long the page's animation frames waited: the store publishes on the next frame, or after
 *   `PUBLISH_FALLBACK_MS` without one (WEB-U-278), so a frame that waited longer means the timer published;
 * - at report time, the page's paint entries and any finished `…/events` Resource Timing entry (an open stream has
 *   none: the browser adds one only when a response ends).
 */
export async function probeStreams(page: Page): Promise<void> {
  await page.addInitScript((fallbackMs: number) => {
    const record: string[] = [];
    const now = () => performance.now().toFixed(0);
    const frames = { requested: 0, ran: 0, longestWaitMs: 0, overFallback: 0 };
    (window as unknown as { __tqaStreams: string[] }).__tqaStreams = record;
    (window as unknown as { __tqaFrames: typeof frames }).__tqaFrames = frames;
    const Native = window.EventSource;
    class Probed extends Native {
      constructor(url: string | URL, init?: EventSourceInit) {
        super(url, init);
        const note = (what: string) =>
          record.push(`${now()} ${what} ${String(url)}`);
        note("created");
        this.addEventListener("open", () => note("open"));
        this.addEventListener("message", () => note("first message"), {
          once: true,
        });
        this.addEventListener("error", () =>
          note(`error readyState=${String(this.readyState)}`),
        );
        const close = this.close.bind(this);
        this.close = () => {
          note("closed");
          close();
        };
      }
    }
    window.EventSource = Probed;
    const fetchNative = window.fetch.bind(window);
    window.fetch = (input, init) => {
      const url =
        typeof input === "string"
          ? input
          : input instanceof URL
            ? input.href
            : input.url;
      if (url.includes("/messages/queued"))
        record.push(`${now()} held request ${url}`);
      return fetchNative(input, init);
    };
    const frameNative = window.requestAnimationFrame.bind(window);
    window.requestAnimationFrame = (callback) => {
      frames.requested += 1;
      const asked = performance.now();
      return frameNative((time) => {
        frames.ran += 1;
        const waited = performance.now() - asked;
        frames.longestWaitMs = Math.max(
          frames.longestWaitMs,
          Math.round(waited),
        );
        // A frame that waited past the store's own fallback: the store's timer published instead.
        if (waited > fallbackMs) frames.overFallback += 1;
        callback(time);
      });
    };
  }, PUBLISH_FALLBACK_MS);
}

/** What the page recorded, with its paints, its frames and any finished stream timing. */
async function readRecord(page: Page): Promise<string[]> {
  return page
    .evaluate(() => {
      const w = window as unknown as {
        __tqaStreams?: string[];
        __tqaFrames?: Record<string, number>;
      };
      const paints = performance
        .getEntriesByType("paint")
        .map((entry) => `${entry.startTime.toFixed(0)} paint ${entry.name}`);
      const streams = performance
        .getEntriesByType("resource")
        .filter((entry) => entry.name.includes("/events"))
        .map((entry) => {
          const timing = entry as PerformanceResourceTiming;
          return `${timing.startTime.toFixed(0)} events timing responseStart=${timing.responseStart.toFixed(0)} ${timing.name}`;
        });
      return [
        ...(w.__tqaStreams ?? []),
        ...paints,
        ...streams,
        `frames ${JSON.stringify(w.__tqaFrames ?? {})}`,
      ];
    })
    .catch(() => ["(the page could not be read)"]);
}

/**
 * Attaches the page's stream record to a test that failed, or to every run with `TQA_STREAM_PROBE_ALWAYS=1` (the
 * diagnostic run under load); does nothing on a pass otherwise.
 */
export async function reportStreams(page: Page, info: TestInfo): Promise<void> {
  const always = process.env.TQA_STREAM_PROBE_ALWAYS === "1";
  if (!always && info.status === info.expectedStatus) return;
  const record = await readRecord(page);
  await info.attach("event streams", {
    body: [page.url(), ...record].join("\n"),
    contentType: "text/plain",
  });
}
