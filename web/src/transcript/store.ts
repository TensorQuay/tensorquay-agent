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
/**
 * How long the stream may still deliver a run's end after the task's record said the run ended, before the page reads
 * the events itself (WEB-U-468): a proxy that buffers the stream opened it and delivered the run's start, then held its
 * end back, so the header read Done while the transcript stayed on "Working…" with Stop and nothing gave Send back.
 */
export const CATCH_UP_WAIT_MS = 3_000;

/** A page of the task's stored events, as the events route answers without `stream=1`. */
type EventPage = { items: unknown[]; next_after: number | null };
/** What the page needs of the task's record: whether a run is going, and the record's last activity. */
type Record = { run_status?: string | undefined; last_activity_at: string };
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
  /** Reads the events after a sequence without the stream, a page at a time; without it the page never catches up. */
  read?: (after: number) => Promise<unknown>;
  /** Reads the task's record again, when an open stream goes silent during a run. */
  readRecord?: () => Promise<unknown>;
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
  /** The task's record as the page last read it, and the catch-up it may have started (WEB-U-468). */
  private stored: Record | null = null;
  private catchingUp: number | null = null;
  /** The wait for a frame on an open stream while a run goes, and how many such waits passed in a row. */
  private quiet: number | null = null;
  private silences = 0;
  private reading = false;

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

  /**
   * The task's record, each time the page reads it: a run the record says ended (its last activity at or after the
   * run's start, WEB-U-414) that the stream still shows going starts the catch-up wait; the run's end arriving in time
   * stops it.
   */
  record(status: string | undefined, at: string): void {
    this.stored = { run_status: status, last_activity_at: at };
    this.judge();
  }

  private judge(): void {
    if (this.disposed || this.catchingUp !== null || this.reading) return;
    if (!this.behind()) return;
    const wait =
      this.dependencies.wait ?? ((run, ms) => window.setTimeout(run, ms));
    this.catchingUp = wait(() => {
      this.catchingUp = null;
      if (this.behind()) void this.catchUp();
    }, CATCH_UP_WAIT_MS);
  }

  /** The record says the run ended and the stream hasn't delivered its end. */
  private behind(): boolean {
    const { run, runs } = this.snapshot.transcript;
    const stored = this.stored;
    if (!stored || run.status !== "running" || !run.id) return false;
    const status = stored.run_status;
    if (!status || status === "running" || status === "idle") return false;
    const started = runs[run.id];
    return (
      started !== undefined &&
      Date.parse(started.startedAt) <= Date.parse(stored.last_activity_at)
    );
  }

  /**
   * Reads the events the stream hasn't delivered, from the last one applied, a page at a time, then opens the stream
   * again from where the read ended. A read that fails is a restart (the open wait's budget).
   */
  private async catchUp(): Promise<void> {
    const read = this.dependencies.read;
    if (!read || this.reading || this.disposed) return;
    this.reading = true;
    this.stopCatchUp();
    this.stopQuiet();
    this.update({ ...this.snapshot, connection: "reconnecting" });
    let after = this.snapshot.transcript.lastSeq;
    let failed = false;
    try {
      for (;;) {
        const page = (await read(after)) as EventPage;
        if (this.disposed) return;
        for (const item of page.items) this.receive(JSON.stringify(item));
        if (page.next_after === null || page.next_after <= after) break;
        after = page.next_after;
      }
    } catch {
      failed = true;
    } finally {
      this.reading = false;
    }
    if (this.disposed) return;
    if (failed) this.recover();
    else {
      this.update({ ...this.snapshot, connection: "reconnecting" });
      this.open(true);
    }
  }

  private stopCatchUp(): void {
    if (this.catchingUp === null) return;
    const stop =
      this.dependencies.stopWaiting ??
      ((handle) => window.clearTimeout(handle));
    stop(this.catchingUp);
    this.catchingUp = null;
  }

  /**
   * An open stream silent while a run goes: after the open wait's time (doubling to its cap, jittered) the record is
   * read again, and a run it says ended is caught up on; one it says running waits twice as long. Never a steady
   * poll, and silence alone never reads the events (a long step is quiet).
   */
  private watchQuiet(): void {
    this.stopQuiet();
    const readRecord = this.dependencies.readRecord;
    if (!readRecord || this.snapshot.transcript.run.status !== "running")
      return;
    const wait =
      this.dependencies.wait ?? ((run, ms) => window.setTimeout(run, ms));
    this.quiet = wait(() => {
      this.quiet = null;
      if (this.disposed || this.snapshot.transcript.run.status !== "running")
        return;
      this.silences += 1;
      void readRecord()
        .then((record) => {
          if (this.disposed) return;
          const { run_status, last_activity_at } = record as Record;
          this.stored = { run_status, last_activity_at };
          if (this.behind()) void this.catchUp();
          else this.watchQuiet();
        })
        .catch(() => {
          if (!this.disposed) this.watchQuiet();
        });
    }, this.backOff(this.silences));
  }

  private stopQuiet(): void {
    if (this.quiet === null) return;
    const stop =
      this.dependencies.stopWaiting ??
      ((handle) => window.clearTimeout(handle));
    stop(this.quiet);
    this.quiet = null;
  }

  /** The open wait's schedule: doubling from its base to its cap, each varied by up to a fifth. */
  private backOff(times: number): number {
    const random = this.dependencies.random ?? Math.random;
    const base = Math.min(
      STREAM_OPEN_WAIT_MS * 2 ** times,
      STREAM_OPEN_MAX_WAIT_MS,
    );
    return Math.round(
      base * (1 - STREAM_OPEN_JITTER + 2 * STREAM_OPEN_JITTER * random()),
    );
  }

  dispose(): void {
    this.disposed = true;
    this.close();
    this.stopCatchUp();
    this.stopQuiet();
    this.stopPublishing();
    // Subscribers stay: they unsubscribe themselves (useSyncExternalStore does, on unmount), and a store connected
    // again after dispose (an effect re-run on the same store) must still notify them.
  }

  private close(): void {
    this.stopWait();
    this.stopQuiet();
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
      this.silences = 0;
      this.watchQuiet();
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
    const ms = this.backOff(this.restarts);
    this.waiting = wait(() => {
      this.waiting = null;
      if (this.disposed || this.source !== source) return;
      // A proxy can hold the headers back too: the events are read without the stream before it is opened again,
      // so the page shows the run meanwhile (WEB-U-468).
      if (this.dependencies.read) void this.catchUp();
      else this.recover();
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
      if (transcript.run.status !== "running") this.stopCatchUp();
      if (!this.reading) {
        this.silences = 0;
        this.watchQuiet();
      }
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
