import { describe, expect, it } from "vitest";

import { english } from "../i18n/translate";
import { frame, ready, runId } from "../test/frames";

import { apply, applyAt, emptyTranscript } from "./reducer";
import { isWorking, runEnded, runPresentation } from "./run-status";

describe("WEB-U-001 human run and environment status", () => {
  it("uses recorded completion time and a live elapsed time while working", () => {
    const started = frame(1, { type: "run.started", run_id: runId });
    const running = apply(emptyTranscript(), started);
    expect(
      runPresentation(running, Date.parse(started.ts) + 22300),
    ).toMatchObject({
      label: "Working… · 22 s",
      environment: null,
    });
    const finished = apply(running, {
      ...frame(2, { type: "run.completed", run_id: runId }),
      ts: new Date(Date.parse(started.ts) + 22300).toISOString(),
    });
    // "Done" alone; how long it took waits in the tooltip.
    expect(
      runPresentation(finished, Date.parse(started.ts) + 90000),
    ).toMatchObject({ label: "Done", took: "Took 22 s" });
  });

  it("follows ready busy, idle and destroyed without an idle environment while working", () => {
    const update = (state: typeof ready.sandbox.state) => ({
      ...ready,
      sandbox: { ...ready.sandbox, state },
    });
    let transcript = apply(emptyTranscript(), update("busy"));
    expect(runPresentation(transcript, 0).label).toContain("Working…");
    transcript = apply(transcript, update("idle"));
    expect(runPresentation(transcript, 0)).toMatchObject({
      label: "Ready",
      environment: null,
    });
    transcript = apply(transcript, update("destroyed"));
    expect(runPresentation(transcript, 0).environment).toBeNull();
    transcript = apply(
      transcript,
      frame(1, { type: "run.started", run_id: runId }),
    );
    for (const state of ["busy", "idle", "destroyed"] as const) {
      transcript = apply(transcript, update(state));
      const status = runPresentation(transcript, 0);
      expect(status.label).toContain("Working…");
      expect(status.environment).toBeNull();
      expect(status.label).not.toMatch(/sandbox|idle|sleeping/i);
    }
  });

  it("names stopped and failed work and only exposes relevant environment transitions", () => {
    const state = apply(
      emptyTranscript(),
      frame(1, { type: "run.started", run_id: runId }),
    );
    expect(
      runPresentation(
        apply(state, frame(2, { type: "run.cancelled", run_id: runId })),
        0,
      ).label,
    ).toBe("Stopped");
    const failed = apply(
      state,
      frame(2, {
        type: "run.failed",
        run_id: runId,
        code: "io",
        reason: "Command failed",
      }),
    );
    // The owner's Mac test: the plain words for the code, never the recorded reason.
    expect(runPresentation(failed, 0).label).toBe(
      "Failed: The task's environment couldn't be reached",
    );
    // A failure code with its own plain copy replaces the runtime's wording.
    const unreadable = apply(
      state,
      frame(2, {
        type: "run.failed",
        run_id: runId,
        code: "fallback_parse",
        reason: "fallback parse failed after retry",
      }),
    );
    expect(runPresentation(unreadable, 0).label).toBe(
      "Failed: The model's answer couldn't be used",
    );
    expect(
      runPresentation({ ...emptyTranscript(), sandbox: "creating" }, 0).label,
    ).toBe("Starting…");
  });

  it("does not call a failed start Ready", () => {
    expect(
      runPresentation({ ...emptyTranscript(), sandbox: "failed" }, 0),
    ).toMatchObject({ label: "Not sent", tone: "danger" });
  });

  it.each(["completed", "cancelled", "failed"] as const)(
    "lets a %s run override stale busy environment state",
    (status) => {
      const transcript = {
        ...emptyTranscript(),
        sandbox: "busy" as const,
        run: { id: runId, status },
      };
      expect(runPresentation(transcript, 0).label).toMatch(
        /^(Done|Stopped|Failed:)/,
      );
    },
  );
});

// A sandbox that keeps stopping is replaced after a growing delay, then not at all (ADR-0016; the gateway's
// SandboxSummary.recovery): the status line says how often it stopped and when it restarts (WEB-U-068).
describe("WEB-U-068 sandbox recovery in the run status", () => {
  const T0 = Date.parse("2026-09-28T08:00:00Z");
  const lost = (
    state: typeof ready.sandbox.state,
    losses: number,
    retry: number | null,
  ) => ({
    ...ready,
    sandbox: {
      ...ready.sandbox,
      state,
      recovery: { losses, retry_in_secs: retry },
    },
  });

  it("counts down to the replacement from when the notice arrived", () => {
    const state = applyAt(emptyTranscript(), lost("destroyed", 2, 15), T0);
    expect(runPresentation(state, T0).environment).toBe(
      "Environment stopped 2 times; restarting in 15 s",
    );
    expect(runPresentation(state, T0 + 5_200).environment).toBe(
      "Environment stopped 2 times; restarting in 10 s",
    );
    expect(runPresentation(state, T0 + 20_000).environment).toBe(
      "Environment stopped 2 times; restarting now",
    );
  });

  it("shows the recovery beside Starting…, and says once for a first loss", () => {
    const state = applyAt(emptyTranscript(), lost("creating", 1, 0), T0);
    expect(runPresentation(state, T0)).toMatchObject({
      word: "Starting…",
      environment: "Environment stopped once; restarting now",
    });
  });

  it("says when the gateway won't replace it, and what brings it back", () => {
    const state = applyAt(emptyTranscript(), lost("failed", 5, null), T0);
    expect(runPresentation(state, T0).environment).toBe(
      "Environment stopped 5 times; it won't restart by itself. Send a message to try again.",
    );
  });

  it("forgets the recovery once the environment is back, or a notice carries none", () => {
    const lostOnce = applyAt(emptyTranscript(), lost("destroyed", 2, 15), T0);
    expect(applyAt(lostOnce, ready, T0).recovery).toBeNull();
    const back = apply(
      lostOnce,
      frame(1, { type: "sandbox.state", state: "busy" }),
    );
    expect(back.recovery).toBeNull();
  });

  it("shows beside Working… when the environment stops mid-run, since the run stays unfinished", () => {
    const started = frame(1, { type: "run.started", run_id: runId });
    const state = applyAt(
      apply(emptyTranscript(), started),
      lost("destroyed", 2, 15),
      T0,
    );
    expect(runPresentation(state, T0)).toMatchObject({
      word: "Working…",
      environment: "Environment stopped 2 times; restarting in 15 s",
    });
  });

  it("reads Starting… rather than Ready while a replacement is pending with no run", () => {
    const state = applyAt(emptyTranscript(), lost("destroyed", 1, 5), T0);
    expect(runPresentation(state, T0).word).toBe("Starting…");
  });

  it("without a clock (a replay), shows the seconds the notice gave", () => {
    const state = apply(emptyTranscript(), lost("destroyed", 3, 45));
    expect(runPresentation(state, T0).environment).toBe(
      "Environment stopped 3 times; restarting in 45 s",
    );
  });
});

// Found before 17.0: "Working… · 4 s" after a minute. The run's start is stamped by the runtime's clock, the count by
// the reader's; when they differ the count stalls or races. The transcript learns the difference from live frames.
describe("WEB-U-122 the live count on the reader's clock", () => {
  const started = frame(1, { type: "run.started", run_id: runId });
  const stamp = Date.parse(started.ts);

  it.each([
    ["ahead of the reader's", -50_000],
    ["behind the reader's", 40_000],
    ["the same as the reader's", 0],
  ])("counts a minute as a minute with the runtime's clock %s", (_, skew) => {
    // Received 200 ms after it was stamped, on a reader's clock `skew` ms from the runtime's.
    const received = stamp + skew + 200;
    const running = applyAt(emptyTranscript(), started, received);
    expect(runPresentation(running, received + 60_000).elapsed).toBe("1 min");
  });

  it("learns from the frame that arrived soonest, not from one replayed late", () => {
    const later = frame(2, { type: "step.started", step: 1, attempt: 1 }, 1);
    // A replay delivers the start 30 s after it happened, then a live frame arrives 100 ms after its stamp.
    let running = applyAt(emptyTranscript(), started, stamp + 30_000);
    running = applyAt(running, later, Date.parse(later.ts) + 100);
    expect(runPresentation(running, stamp + 100 + 45_000).elapsed).toBe("45 s");
  });

  it("never counts below zero", () => {
    const running = applyAt(emptyTranscript(), started, stamp);
    expect(runPresentation(running, stamp - 5_000).elapsed).toBe("0 s");
  });
});

// WEB-U-414 (17.0's release candidate; a live check before 17.0): a fresh page on an ended lead task read
// "Working… · 4 min 46 s" for a moment while the stream replayed, before Done: the replay had its run's start but
// not yet its end. A run that started before the task's record was last active ended as the record says; a run that
// started after it is new, and works.
describe("WEB-U-414 a replayed run the stored status says ended never reads Working", () => {
  const started = frame(1, { type: "run.started", run_id: runId });
  const replaying = [ready, started].reduce(apply, emptyTranscript());
  const now = Date.parse(started.ts) + 286_000;
  it("keeps the stored end while the replay has the run's start but not its end", () => {
    const after = new Date(Date.parse(started.ts) + 290_000).toISOString();
    expect(
      runPresentation(replaying, now, english, "completed", "lead", after),
    ).toMatchObject({ label: "Done", tone: "ok" });
    expect(
      runPresentation(replaying, now, english, "failed", null, after),
    ).toMatchObject({ word: "Failed" });
    expect(runEnded(replaying, "completed", after)).toBe("completed");
    expect(isWorking(replaying, "completed", after)).toBe(false);
  });

  it("works for a run that started after the record was read", () => {
    const before = new Date(Date.parse(started.ts) - 60_000).toISOString();
    expect(
      runPresentation(replaying, now, english, "completed", null, before),
    ).toMatchObject({ label: "Working… · 4 min 46 s" });
    expect(runEnded(replaying, "completed", before)).toBe("running");
    expect(isWorking(replaying, "completed", before)).toBe(true);
  });

  it("works when the record says the run is running", () => {
    const after = new Date(Date.parse(started.ts) + 290_000).toISOString();
    expect(
      runPresentation(replaying, now, english, "running", null, after),
    ).toMatchObject({ label: "Working… · 4 min 46 s" });
  });
});
