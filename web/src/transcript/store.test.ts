import { describe, expect, it, vi } from "vitest";

import { conversationId, frame, transcriptFrames } from "../test/frames";

import { noticeText } from "./notice-copy";
import {
  PUBLISH_FALLBACK_MS,
  STREAM_OPEN_MAX_WAIT_MS,
  STREAM_OPEN_WAIT_MS,
  TranscriptStore,
} from "./store";

class Source extends EventTarget {
  readyState = 1;
  close = vi.fn(() => {
    this.readyState = 2;
  });
  send(data: unknown) {
    this.dispatchEvent(
      new MessageEvent("message", { data: JSON.stringify(data) }),
    );
  }
}

function setup(now: () => number = () => 0) {
  const sources: Source[] = [];
  const urls: string[] = [];
  const scheduled: FrameRequestCallback[] = [];
  const store = new TranscriptStore({
    open: (url) => {
      const source = new Source();
      sources.push(source);
      urls.push(url);
      return source;
    },
    schedule: (callback) => {
      scheduled.push(callback);
      return scheduled.length;
    },
    cancel: vi.fn(),
    now,
    // No real timers: these tests don't concern the wait for a stream to open (WEB-U-160 does).
    wait: () => 0,
    stopWaiting: () => undefined,
  });
  store.connect(conversationId);
  return { store, sources, urls, scheduled };
}

describe("WEB-U-001 animation frame batching", () => {
  it("applies deltas immediately but only notifies once per animation frame", () => {
    const { store, sources, scheduled } = setup();
    const notify = vi.fn();
    store.subscribe(notify);
    sources[0]?.send(frame(1, { type: "text.delta", text: "a" }));
    sources[0]?.send(frame(2, { type: "text.delta", text: "b" }));
    expect(store.getSnapshot().transcript.items[0]).toMatchObject({
      text: "ab",
    });
    expect(notify).not.toHaveBeenCalled();
    expect(scheduled).toHaveLength(1);
    scheduled[0]?.(0);
    expect(notify).toHaveBeenCalledTimes(1);
    store.dispose();
  });
});

describe("WEB-U-002 reconnect and disposal", () => {
  it("closes a gap and requests replay after the last applied sequence", () => {
    const { store, sources, urls } = setup();
    expect(urls[0]).toBe(
      `/api/v1/conversations/${conversationId}/events?stream=1&protocol=1`,
    );
    sources[0]?.send(frame(1, { type: "text.delta", text: "first" }));
    sources[0]?.send(frame(3, { type: "text.delta", text: "gap" }));
    expect(sources[0]?.close).toHaveBeenCalledOnce();
    expect(urls[1]).toBe(
      `/api/v1/conversations/${conversationId}/events?stream=1&protocol=1&after=1`,
    );
    sources[1]?.send(frame(2, { type: "text.delta", text: " second" }));
    sources[1]?.send(frame(3, { type: "text.delta", text: " third" }));
    expect(store.getSnapshot().transcript.items[0]).toMatchObject({
      text: "first second third",
    });
    store.dispose();
  });

  it("lets native EventSource reconnect five times and ignores replay duplicates", () => {
    const { store, sources, urls } = setup();
    for (const frame of transcriptFrames) sources[0]?.send(frame);
    const before = store.getSnapshot().transcript;
    for (let count = 0; count < 5; count++) {
      sources[0]?.dispatchEvent(new Event("error"));
      sources[0]?.dispatchEvent(new Event("open"));
      for (const frame of transcriptFrames.filter((item) => "seq" in item))
        sources[0]?.send(frame);
    }
    expect(urls).toHaveLength(1);
    expect(store.getSnapshot().transcript).toEqual(before);
    store.dispose();
    expect(sources[0]?.close).toHaveBeenCalledOnce();
  });

  // WEB-U-234: a stream error's banner is over once the stream is back after a drop; while it stays open it remains.
  it("clears a stream error's banner when the stream reopens after a drop, and only then", () => {
    const { store, sources } = setup();
    sources[0]?.dispatchEvent(new Event("open"));
    for (const frame of transcriptFrames) sources[0]?.send(frame);
    sources[0]?.send({
      type: "error",
      code: "io",
      message: "",
      retryable: true,
    });
    const raised = store.getSnapshot().transcript.banner;
    expect(raised).not.toBeNull();
    // A second open without a drop leaves it.
    sources[0]?.dispatchEvent(new Event("open"));
    expect(store.getSnapshot().transcript.banner).toEqual(raised);
    sources[0]?.dispatchEvent(new Event("error"));
    expect(store.getSnapshot().connection).toBe("reconnecting");
    sources[0]?.dispatchEvent(new Event("open"));
    expect(store.getSnapshot().connection).toBe("live");
    expect(store.getSnapshot().transcript.banner).toBeNull();
  });

  it("keeps a quota notice across a reconnect: the gateway says it once per run, live only", () => {
    const { store, sources } = setup();
    sources[0]?.dispatchEvent(new Event("open"));
    for (const frame of transcriptFrames) sources[0]?.send(frame);
    sources[0]?.send({
      type: "error",
      code: "workspace_quota",
      message: "",
      retryable: true,
    });
    const notice = store.getSnapshot().transcript.banner;
    expect(notice).not.toBeNull();
    sources[0]?.dispatchEvent(new Event("error"));
    sources[0]?.dispatchEvent(new Event("open"));
    expect(store.getSnapshot().transcript.banner).toEqual(notice);
  });

  it("stops malformed input, ignores detached sources, and exposes an error banner", () => {
    const { store, sources } = setup();
    // A known kind with a bad field; an unknown kind is ignored instead (task 069b).
    sources[0]?.send({ type: "heartbeat", run_id: 5 });
    expect(sources[0]?.close).toHaveBeenCalledOnce();
    const banner = store.getSnapshot().transcript.banner;
    expect(banner && noticeText(banner)).toContain("invalid");
    store.dispose();
    sources[0]?.send(frame(1, { type: "text.delta", text: "too late" }));
    expect(store.getSnapshot().transcript.lastSeq).toBe(0);
  });
});

describe("WEB-U-002 unknown kinds in the live stream", () => {
  it("keeps the stream open without a banner and renders the next known event", () => {
    const { store, sources } = setup();
    sources[0]?.send({ type: "invented", detail: "from a newer gateway" });
    sources[0]?.send({
      type: "event",
      seq: 1,
      run_id: "run-example",
      step: 1,
      ts: "2026-01-01T00:00:00Z",
      event: { type: "memory.pruned", summary: "Earlier steps" },
    });
    sources[0]?.send(frame(2, { type: "text.delta", text: "after" }));
    const snapshot = store.getSnapshot();
    expect(sources[0]?.close).not.toHaveBeenCalled();
    expect(snapshot.transcript.banner).toBeNull();
    expect(snapshot.transcript.lastSeq).toBe(2);
    expect(snapshot.connection).toBe("live");
    store.dispose();
  });
});

describe("SEC-001 blocked network attempts in the live stream", () => {
  it("uses the store clock to show a repeated destination once a minute", () => {
    let clock = 0;
    const { store, sources } = setup(() => clock);
    const denied = {
      type: "error",
      code: "egress_denied",
      message: "Internal detail",
      retryable: false,
      target: { host: "example.invalid", port: 443 },
    };
    sources[0]?.send(denied);
    clock = 30_000;
    sources[0]?.send(denied);
    clock = 60_000;
    sources[0]?.send(denied);
    expect(
      store
        .getSnapshot()
        .transcript.items.filter((item) => item.kind === "notice"),
    ).toHaveLength(2);
    expect(store.getSnapshot().transcript.banner).toBeNull();
    store.dispose();
  });
});

// A store keeps its subscribers across a dispose and a new connect: React re-runs a hook's effect on the same store
// (Strict Mode mounts, unmounts and mounts again), while its useSyncExternalStore subscription stays. Clearing them on
// dispose left the page subscribed to nothing: the stream came back, the page never re-rendered.
// Regression (TEST-PLAN §19, fixed in 348c2b21).
describe("WEB-U-158 a store reconnected after dispose still notifies its subscribers", () => {
  it("notifies the subscriber of frames from the new stream", () => {
    const { store, sources, scheduled } = setup();
    const notify = vi.fn();
    store.subscribe(notify);
    store.dispose();
    store.connect(conversationId);
    const fresh = sources.at(-1);
    expect(fresh).not.toBe(sources[0]);
    fresh?.send(frame(1, { type: "text.delta", text: "back" }));
    scheduled.at(-1)?.(0);
    expect(notify).toHaveBeenCalled();
    store.dispose();
  });
});

/** A store whose streams never open by themselves, on a fake clock: each wait is recorded, and fires on request. */
function silent(random = () => 0.5) {
  const sources: Source[] = [];
  const urls: string[] = [];
  const waits: { run: () => void; ms: number; stopped: boolean }[] = [];
  const store = new TranscriptStore({
    open: (url) => {
      const source = new Source();
      source.readyState = 0;
      sources.push(source);
      urls.push(url);
      return source;
    },
    schedule: (callback) => {
      callback(0);
      return 0;
    },
    cancel: vi.fn(),
    wait: (run, ms) => waits.push({ run, ms, stopped: false }) - 1,
    stopWaiting: (handle) => {
      const wait = waits[handle];
      if (wait) wait.stopped = true;
    },
    random,
  });
  store.connect(conversationId);
  /** Runs the latest wait still pending, as its time coming would. */
  const elapse = () => {
    const pending = waits.filter((wait) => !wait.stopped).at(-1);
    if (!pending) throw new Error("No wait is pending");
    pending.stopped = true;
    pending.run();
  };
  return { store, sources, urls, waits, elapse };
}

// TEST-PLAN §19a and guardrails §5.4 (every socket has back-off and a resync path): a task's stream that never opens
// (no open, no error) left the page waiting for ever, on "Sending…". It is closed and opened again after a bounded
// wait that backs off, and the store gives up in words after its restart limit.
describe("WEB-U-160 a stream that never opens is opened again", () => {
  it("closes a silent stream after the wait and opens another, from the last frame it has", () => {
    const { store, sources, urls, waits, elapse } = silent();
    expect(waits[0]?.ms).toBe(STREAM_OPEN_WAIT_MS);
    elapse();
    expect(sources[0]?.close).toHaveBeenCalled();
    expect(sources).toHaveLength(2);
    expect(urls[1]).toContain("after=0");
    expect(store.getSnapshot().connection).toBe("reconnecting");
    store.dispose();
  });

  it("leaves a stream alone that opens in time", () => {
    const { store, sources, waits } = silent();
    sources[0]?.dispatchEvent(new Event("open"));
    expect(waits[0]?.stopped).toBe(true);
    expect(sources).toHaveLength(1);
    expect(store.getSnapshot().connection).toBe("live");
    store.dispose();
  });

  it("waits twice as long each time, up to its cap", () => {
    const { store, waits, elapse } = silent();
    for (let attempt = 0; attempt < 5; attempt += 1) elapse();
    expect(waits.map((wait) => wait.ms)).toEqual(
      [1, 2, 4, 8, 16, 32].map((factor) =>
        Math.min(STREAM_OPEN_WAIT_MS * factor, STREAM_OPEN_MAX_WAIT_MS),
      ),
    );
    store.dispose();
  });

  it("varies each wait by at most a fifth, so many pages don't retry at once", () => {
    for (const [random, factor] of [
      [0, 0.8],
      [1, 1.2],
    ] as const) {
      const { store, waits } = silent(() => random);
      expect(waits[0]?.ms).toBe(Math.round(STREAM_OPEN_WAIT_MS * factor));
      store.dispose();
    }
  });

  it("says it couldn't reconnect after the restart limit", () => {
    const { store, sources, elapse } = silent();
    for (let attempt = 0; attempt < 6; attempt += 1) elapse();
    const { connection, transcript } = store.getSnapshot();
    expect(connection).toBe("closed");
    expect(transcript.banner ? noticeText(transcript.banner) : "").toBe(
      "The transcript could not reconnect. Reload to retry.",
    );
    expect(sources.at(-1)?.close).toHaveBeenCalled();
    store.dispose();
  });

  it("stops waiting when the page leaves", () => {
    const { store, waits } = silent();
    expect(waits).toHaveLength(1);
    store.dispose();
    expect(waits.every((wait) => wait.stopped)).toBe(true);
  });
});

/** A store whose animation frames come only on request, with its publish timer recorded. */
function framed() {
  const sources: Source[] = [];
  const frames: FrameRequestCallback[] = [];
  const cancelled: number[] = [];
  const timers: { run: () => void; ms: number; stopped: boolean }[] = [];
  const store = new TranscriptStore({
    open: () => {
      const source = new Source();
      sources.push(source);
      return source;
    },
    schedule: (callback) => frames.push(callback),
    cancel: (handle) => cancelled.push(handle),
    wait: () => 0,
    stopWaiting: () => undefined,
    later: (run, ms) => timers.push({ run, ms, stopped: false }) - 1,
    stopLater: (handle) => {
      const timer = timers[handle];
      if (timer) timer.stopped = true;
    },
  });
  store.connect(conversationId);
  const notified = vi.fn();
  store.subscribe(notified);
  return { store, sources, frames, cancelled, timers, notified };
}

// TEST-PLAN §19a (WEB-U-278): snapshots are published once per animation frame, but a page that gets no frame (a
// starved host, an occluded window) still shows what arrived within a quarter of a second, and a frame that never
// comes can't hold every later update back.
describe("WEB-U-278 publishing without waiting for a frame forever", () => {
  it("publishes on the timer when no frame comes, and the next update is scheduled again", () => {
    const { sources, frames, cancelled, timers, notified } = framed();
    sources[0]?.dispatchEvent(new Event("open"));
    expect(notified).not.toHaveBeenCalled();
    expect(timers.at(-1)?.ms).toBe(PUBLISH_FALLBACK_MS);
    timers.at(-1)?.run();
    expect(notified).toHaveBeenCalledTimes(1);
    expect(cancelled).toEqual([1]); // the frame that never came is given up
    sources[0]?.send(frame(1, { type: "text.delta", text: "a" }));
    expect(frames).toHaveLength(2);
    expect(timers).toHaveLength(2);
  });

  it("publishes once on the frame when it comes first, stopping the timer", () => {
    const { sources, frames, timers, notified } = framed();
    sources[0]?.dispatchEvent(new Event("open"));
    sources[0]?.send(frame(1, { type: "text.delta", text: "a" }));
    expect(frames).toHaveLength(1);
    frames[0]?.(0);
    expect(notified).toHaveBeenCalledTimes(1);
    expect(timers.map((timer) => timer.stopped)).toEqual([true]);
  });

  it("keeps publishing when its timer calls back at once", () => {
    const sources: Source[] = [];
    const notified = vi.fn();
    const store = new TranscriptStore({
      open: () => {
        const source = new Source();
        sources.push(source);
        return source;
      },
      schedule: () => 1,
      cancel: vi.fn(),
      wait: () => 0,
      stopWaiting: () => undefined,
      later: (run) => {
        run();
        return 7;
      },
      stopLater: vi.fn(),
    });
    store.connect(conversationId);
    store.subscribe(notified);
    sources[0]?.dispatchEvent(new Event("open"));
    sources[0]?.send(frame(1, { type: "text.delta", text: "a" }));
    expect(notified).toHaveBeenCalledTimes(2); // each update published, none left blocked by a stale timer
  });

  it("stops both when the page leaves", () => {
    const { store, sources, cancelled, timers, notified } = framed();
    sources[0]?.dispatchEvent(new Event("open"));
    store.dispose();
    expect(cancelled).toEqual([1]);
    expect(timers[0]?.stopped).toBe(true);
    expect(notified).not.toHaveBeenCalled();
  });
});
