import { pathFor } from "../api/operations";
import { openEventStream } from "../api/stream";
import { parseFrame } from "../protocol/validate";

import { message, type Copy } from "./notice-copy";
import { PUBLISH_FALLBACK_MS } from "./publish-timing";
import { applyAt, emptyTranscript, SequenceGap } from "./reducer";
import type { Transcript } from "./types";

type Source = EventTarget & { close: () => void; readyState: number };

/**
 * How long a task's stream may stay silent before it is opened again (TEST-PLAN §19a, guardrails §5.4). A stream answers
 * at once when it can (its headers, then the session frame); one that neither opens nor fails this long has stalled,
 * and waiting longer only leaves the page on "Sending…". Each restart waits twice as long, up to the cap.
 */
export const STREAM_OPEN_WAIT_MS = 8_000;
export const STREAM_OPEN_MAX_WAIT_MS = 60_000;
export { PUBLISH_FALLBACK_MS };
/** Each wait varies by up to a fifth either way, so many pages that lost their streams at once don't retry at once. */
const STREAM_OPEN_JITTER = 0.2;
type Dependencies = {
  open: (url: string) => Source;
  schedule: typeof requestAnimationFrame;
  cancel: typeof cancelAnimationFrame;
  onClosed?: () => void;
  /** Wall-clock milliseconds for live-only limits; defaults to Date.now. */
  now?: () => number;
  /** Runs `run` after `ms` (a timer); defaults to the browser's. */
  wait?: (run: () => void, ms: number) => number;
  stopWaiting?: (handle: number) => void;
  /** A number in [0, 1) for the waits' jitter; defaults to Math.random. */
  random?: () => number;
  /** Runs `run` after `ms`, for the publish's timer; defaults to the browser's. */
  later?: (run: () => void, ms: number) => number;
  stopLater?: (handle: number) => void;
};
type Snapshot = {
  transcript: Transcript;
  connection: "connecting" | "live" | "reconnecting" | "closed";
};

/** Applies every frame immediately while publishing at most once per browser animation frame. */
export class TranscriptStore {
  private snapshot: Snapshot = {
    transcript: emptyTranscript(),
    connection: "connecting",
  };
  private readonly initial = this.snapshot;
  private readonly listeners = new Set<() => void>();
  private source: Source | null = null;
  private detach: (() => void) | null = null;
  private pending: number | null = null;
  /** The publish's timer, beside the pending frame. */
  private fallback: number | null = null;
  /** The wait for the current stream to open, while it hasn't. */
  private waiting: number | null = null;
  private conversation = "";
  private restarts = 0;
  private disposed = false;

  constructor(
    private readonly dependencies: Dependencies = {
      open: openEventStream,
      schedule: (callback) => requestAnimationFrame(callback),
      cancel: (handle) => cancelAnimationFrame(handle),
    },
  ) {}

  getSnapshot = (): Snapshot => this.snapshot;
  getServerSnapshot = (): Snapshot => this.initial;
  subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  };

  connect(conversation: string): void {
    this.disposed = false;
    if (this.conversation !== conversation) {
      this.snapshot = {
        transcript: emptyTranscript(),
        connection: "connecting",
      };
      this.restarts = 0;
    }
    this.conversation = conversation;
    this.open(false);
  }

  dispose(): void {
    this.disposed = true;
    this.close();
    this.stopPublishing();
    // Subscribers stay: they unsubscribe themselves (useSyncExternalStore does, on unmount), and a store connected
    // again after dispose (an effect re-run on the same store) must still notify them.
  }

  private close(): void {
    this.stopWait();
    this.detach?.();
    this.detach = null;
    this.source?.close();
    this.source = null;
  }

  private open(resume: boolean): void {
    this.close();
    const query = new URLSearchParams({ stream: "1", protocol: "1" });
    if (resume) query.set("after", String(this.snapshot.transcript.lastSeq));
    const url = pathFor("events", { id: this.conversation }, query);
    const source = this.dependencies.open(url);
    this.source = source;
    const message = (event: Event) => {
      if (
        !(event instanceof MessageEvent) ||
        this.disposed ||
        this.source !== source
      )
        return;
      const data: unknown = event.data;
      this.receive(data);
    };
    const opened = () => {
      this.stopWait();
      // Back after a drop: a banner a stream error raised before it is over, as the reconnect line is; the replay
      // shows what happened (UI-GUIDELINES §14, WEB-U-234). A session.ready alone isn't a reconnect: the gateway sends
      // one on every sandbox change. A quota notice stays even then: the gateway says it once per run, live only.
      const { transcript } = this.snapshot;
      const back =
        this.snapshot.connection === "reconnecting" &&
        transcript.bannerEndsOnReconnect;
      this.update({
        transcript: back
          ? { ...transcript, banner: null, bannerEndsOnReconnect: false }
          : transcript,
        connection: "live",
      });
    };
    const failed = () => {
      const closed = source.readyState === 2;
      this.update({
        ...this.snapshot,
        connection: closed ? "closed" : "reconnecting",
      });
      if (closed) this.dependencies.onClosed?.();
    };
    source.addEventListener("message", message);
    source.addEventListener("open", opened);
    source.addEventListener("error", failed);
    this.detach = () => {
      source.removeEventListener("message", message);
      source.removeEventListener("open", opened);
      source.removeEventListener("error", failed);
    };
    this.awaitOpen(source);
  }

  /** Opens the stream again if it neither opens nor fails in time, waiting longer after each restart. */
  private awaitOpen(source: Source): void {
    const wait =
      this.dependencies.wait ?? ((run, ms) => window.setTimeout(run, ms));
    const random = this.dependencies.random ?? Math.random;
    const base = Math.min(
      STREAM_OPEN_WAIT_MS * 2 ** this.restarts,
      STREAM_OPEN_MAX_WAIT_MS,
    );
    const ms = Math.round(
      base * (1 - STREAM_OPEN_JITTER + 2 * STREAM_OPEN_JITTER * random()),
    );
    this.waiting = wait(() => {
      this.waiting = null;
      if (this.disposed || this.source !== source) return;
      this.recover();
    }, ms);
  }

  private stopWait(): void {
    if (this.waiting === null) return;
    const stop =
      this.dependencies.stopWaiting ??
      ((handle) => window.clearTimeout(handle));
    stop(this.waiting);
    this.waiting = null;
  }

  private receive(data: unknown): void {
    try {
      if (typeof data !== "string") throw new Error("Invalid event"); // copy-lint: not user copy
      const frame = parseFrame(data);
      const previous = this.snapshot.transcript;
      const transcript = applyAt(
        previous,
        frame,
        (this.dependencies.now ?? Date.now)(),
      );
      if (transcript.lastSeq > previous.lastSeq) this.restarts = 0;
      this.update({ transcript, connection: "live" });
      if (frame.type === "error" && frame.code === "seq_gap") this.recover();
    } catch (error) {
      if (error instanceof SequenceGap) this.recover();
      else this.fail(message("stream.invalid"));
    }
  }

  private recover(): void {
    if (++this.restarts > 5) {
      this.fail(message("stream.reconnectFailed"));
      return;
    }
    this.update({ ...this.snapshot, connection: "reconnecting" });
    this.open(true);
  }

  private fail(copy: Copy): void {
    this.close();
    this.update({
      transcript: {
        ...this.snapshot.transcript,
        banner: copy,
        bannerEndsOnReconnect: false,
      },
      connection: "closed",
    });
  }

  private update(snapshot: Snapshot): void {
    this.snapshot = snapshot;
    if (this.pending !== null || this.fallback !== null || this.disposed)
      return;
    const later =
      this.dependencies.later ?? ((run, ms) => window.setTimeout(run, ms));
    // Either may call back at once (tests' do): each is pending only if it hasn't run yet.
    let timed = false;
    const timer = later(() => {
      timed = true;
      this.fallback = null;
      this.publish();
    }, PUBLISH_FALLBACK_MS);
    if (timed) return;
    this.fallback = timer;
    let ran = false;
    const handle = this.dependencies.schedule(() => {
      ran = true;
      this.pending = null;
      this.publish();
    });
    if (!ran) this.pending = handle;
  }

  /** Tells every subscriber, once, whichever of the frame and the timer came first; the other is stopped. */
  private publish(): void {
    this.stopPublishing();
    for (const listener of this.listeners) listener();
  }

  private stopPublishing(): void {
    if (this.pending !== null) this.dependencies.cancel(this.pending);
    this.pending = null;
    if (this.fallback !== null) {
      const stop =
        this.dependencies.stopLater ??
        ((handle) => window.clearTimeout(handle));
      stop(this.fallback);
    }
    this.fallback = null;
  }
}
