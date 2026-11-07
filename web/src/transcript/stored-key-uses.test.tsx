import { render, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import type { Event } from "../protocol/generated/protocol";
import { parseFrame } from "../protocol/validate";
import { frame, runId } from "../test/frames";

import { apply, applyAt, emptyTranscript } from "./reducer";
import {
  KEY_USE_SKEW_MS,
  placeKeyUses,
  type StoredKeyUse,
} from "./stored-key-uses";
import { TranscriptView } from "./TranscriptView";
import type { Transcript } from "./types";

const ME = "7d1a6b0e-3c2f-4a5b-9c8d-0e1f2a3b4c5d";
const OTHER = "0f9e8d7c-6b5a-4e3d-8c2b-1a0f9e8d7c6b";
const START = Date.UTC(2026, 0, 1);
/** A moment `seconds` into the run, as the runtime or the gateway stamps it. */
const at = (seconds: number) => new Date(START + seconds * 1000).toISOString();

/** A frame stamped by the runtime `seconds` into the run. */
const stamped = (seq: number, event: Event, seconds: number, step = 1) => ({
  ...frame(seq, event, step),
  ts: at(seconds),
});

/** A finished run whose steps started at 0, 10 and 20 s (step 2 retried at 14 s), ending at 30 s. */
function finished(): Transcript {
  return [
    stamped(1, { type: "run.started", run_id: runId }, 0),
    stamped(2, { type: "step.started", step: 1, attempt: 1 }, 0),
    stamped(3, { type: "step.started", step: 2, attempt: 1 }, 10, 2),
    stamped(4, { type: "step.started", step: 2, attempt: 2 }, 14, 2),
    stamped(5, { type: "step.started", step: 3, attempt: 1 }, 20, 3),
    stamped(6, { type: "run.completed", run_id: runId }, 30, 3),
  ].reduce(apply, emptyTranscript());
}

/** A run of one step, still going. */
function oneStep(): Transcript {
  return [
    stamped(1, { type: "run.started", run_id: runId }, 0),
    stamped(2, { type: "step.started", step: 1, attempt: 1 }, 0),
  ].reduce(apply, emptyTranscript());
}

/** A stored row of the task's key uses (GET …/key-uses), its calls between `from` and `to` seconds. */
const row = (
  from: number,
  to = from,
  extra: Partial<StoredKeyUse> = {},
): StoredKeyUse => ({
  id: `use-${from}-${to}`,
  run_id: runId,
  sender_id: ME,
  name: "GITHUB_WORK",
  host: "api.github.com",
  port: 443,
  outcome: "ok",
  count: 1,
  first_at: at(from),
  last_at: at(to),
  ...extra,
});

const use = {
  sender: ME,
  name: "GITHUB_WORK",
  host: "api.github.com",
  port: 443,
  outcome: "ok",
};
const keysOf = (state: Transcript, step: number) => {
  const item = state.items.find(
    (entry) => entry.kind === "step" && entry.step === step,
  );
  return item?.kind === "step" ? (item.keys ?? []) : [];
};
const runKeys = (state: Transcript) => state.runs[runId]?.keys ?? [];
const onSteps = (state: Transcript) =>
  [1, 2, 3].flatMap((step) => keysOf(state, step));

// ADR-0037: a page opened later reads the task's key uses by run. The steps' times are the runtime's and the rows'
// the gateway's, so a row goes on a step only when it lies clearly inside it; otherwise once at the run's level.
describe("WEB-U-193 a task's stored key uses, placed at load", () => {
  it("puts a row that lies inside a step on that step", () => {
    const placed = placeKeyUses(finished(), [row(22, 25)]);
    expect(keysOf(placed, 3)).toEqual([use]);
    expect(keysOf(placed, 1)).toEqual([]);
    expect(runKeys(placed)).toEqual([]);
    // The run ended at 30 s on the runtime's clock; the gateway's may read a little later.
    expect(keysOf(placeKeyUses(finished(), [row(31)]), 3)).toEqual([use]);
  });

  it("says a row near a step's boundary at the run's level, never on a guessed step", () => {
    const near = KEY_USE_SKEW_MS / 1000 / 2;
    for (const placed of [
      // Just after step 3 started, or just before it: either clock may be ahead.
      placeKeyUses(finished(), [row(20 + near)]),
      placeKeyUses(finished(), [row(20 - near)]),
      // Calls that span two steps.
      placeKeyUses(finished(), [row(16, 22)]),
    ]) {
      expect(onSteps(placed)).toEqual([]);
      expect(runKeys(placed)).toEqual([use]);
    }
  });

  it("measures a retried step from its first attempt", () => {
    // Step 2's first attempt began at 10 s; its retry at 14 s. A call at 12 s is step 2's, not step 1's.
    const placed = placeKeyUses(finished(), [row(12)]);
    expect(keysOf(placed, 2)).toEqual([use]);
    expect(keysOf(placed, 1)).toEqual([]);
  });

  it("says a row outside the run's steps at the run's level", () => {
    const placed = placeKeyUses(finished(), [row(-30), row(45)]);
    expect(onSteps(placed)).toEqual([]);
    expect(runKeys(placed)).toEqual([use]);
  });

  it("puts any row of a one-step run on its step", () => {
    const placed = placeKeyUses(oneStep(), [
      row(0),
      row(600, 600, { outcome: "expired" }),
    ]);
    expect(keysOf(placed, 1)).toEqual([use, { ...use, outcome: "expired" }]);
    expect(runKeys(placed)).toEqual([]);
  });

  it("lets the last step of a running run run on", () => {
    const running = finished();
    const placed = placeKeyUses(
      {
        ...running,
        runs: { [runId]: { ...running.runs[runId]!, finishedAt: null } },
      },
      [row(500)],
    );
    expect(keysOf(placed, 3)).toEqual([use]);
  });

  it("never says a use twice: a live line or another row with the same key, address and outcome", () => {
    const seen = applyAt(
      finished(),
      parseFrame(
        JSON.stringify({
          type: "host_key.used",
          run_id: runId,
          step: 1,
          sender_id: ME,
          name: "GITHUB_WORK",
          host: "api.github.com",
          port: 443,
        }),
      ),
      1_000,
    );
    // The live line is on step 1; the stored row of the same use, wherever it falls, adds nothing.
    const placed = placeKeyUses(seen, [row(22), row(20)]);
    expect(onSteps(placed)).toEqual([use]);
    expect(runKeys(placed)).toEqual([]);
    // Another outcome is another line; two rows of it at the run's level are one.
    const other = placeKeyUses(finished(), [
      // Near step 3's start, and after the run's end: neither on a step.
      row(21, 21, { outcome: "foreign_run", id: "a" }),
      row(33, 33, { outcome: "foreign_run", id: "b" }),
    ]);
    expect(runKeys(other)).toEqual([{ ...use, outcome: "foreign_run" }]);
  });

  // ADR-0037 (c740e186): someone removed from a shared key mid-run still gets its name in that run's live refusal, while
  // the stored row, judged when read, has none. The same use, said once.
  it("takes a live line with the key's name and its stored row without one as the same use", () => {
    const seen = applyAt(
      finished(),
      parseFrame(
        JSON.stringify({
          type: "host_key.refused",
          outcome: "revoked",
          run_id: runId,
          step: 3,
          sender_id: ME,
          name: "GITHUB_WORK",
          host: "api.github.com",
          port: 443,
        }),
      ),
      1_000,
    );
    const placed = placeKeyUses(seen, [
      row(22, 22, { name: null, outcome: "revoked" }),
      row(5, 5, { name: null, outcome: "revoked" }),
    ]);
    expect(onSteps(placed)).toEqual([{ ...use, outcome: "revoked" }]);
    expect(runKeys(placed)).toEqual([]);
  });

  it("says a refusal made while no run was going at the task's level, and drops a row of a run not on the page", () => {
    const placed = placeKeyUses(finished(), [
      row(90, 90, { run_id: null, outcome: "foreign_run" }),
      row(5, 5, { run_id: "run-elsewhere" }),
    ]);
    expect(placed.taskKeys).toEqual([{ ...use, outcome: "foreign_run" }]);
    expect(onSteps(placed)).toEqual([]);
    expect(runKeys(placed)).toEqual([]);
  });

  it("places the oldest first, as the gateway lists the newest first", () => {
    const placed = placeKeyUses(finished(), [
      row(26, 26, { host: "b.example" }),
      row(22, 22, { host: "a.example" }),
    ]);
    expect(keysOf(placed, 3).map(({ host }) => host)).toEqual([
      "a.example",
      "b.example",
    ]);
  });

  it("shows run and task lines in words, the name to the sender alone, an IPv6 address bracketed", () => {
    const placed = placeKeyUses(finished(), [
      row(16, 22, { host: "2001:db8::34", port: 8443 }),
      row(90, 90, { run_id: null, outcome: "foreign_run" }),
    ]);
    const { unmount } = render(
      <TranscriptView transcript={placed} viewer={ME} />,
    );
    const run = screen.getByText("During this run").closest("section");
    expect(run).not.toBeNull();
    expect(
      within(run!).getByText("Called [2001:db8::34]:8443 with GITHUB_WORK", {
        exact: true,
      }),
    ).toBeInTheDocument();
    expect(screen.getByText("When no run was going")).toBeInTheDocument();
    expect(
      screen.getByText(
        "GITHUB_WORK wasn't sent to api.github.com: it came from an earlier run, and each run has its own.",
        { exact: true },
      ),
    ).toBeInTheDocument();
    unmount();
    render(<TranscriptView transcript={placed} viewer={OTHER} />);
    expect(
      screen.getByText("Called [2001:db8::34]:8443 with a key", {
        exact: true,
      }),
    ).toBeInTheDocument();
    expect(screen.queryByText(/GITHUB_WORK/)).toBeNull();
  });

  // The gateway names a key only to someone who may still use it (a teammate's placeholder, or a shared key they were
  // removed from): their own row then has no name, and reads as anyone else's.
  it("reads a row the gateway left unnamed as a key, to its own sender too", () => {
    const placed = placeKeyUses(finished(), [
      row(22, 22, { name: null }),
      row(24, 24, { name: null, outcome: "foreign_sandbox" }),
    ]);
    render(<TranscriptView transcript={placed} viewer={ME} />);
    expect(
      screen.getByText("Called api.github.com with a key", { exact: true }),
    ).toBeInTheDocument();
    expect(
      screen.getByText(
        "A key wasn't sent to api.github.com: it came from another task.",
        { exact: true },
      ),
    ).toBeInTheDocument();
  });
});
