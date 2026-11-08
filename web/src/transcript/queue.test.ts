import { describe, expect, it } from "vitest";

import { parseFrame } from "../protocol/validate";
import { ready, transcriptFrames } from "../test/frames";

import { apply, emptyTranscript } from "./reducer";
import { runPresentation } from "./run-status";

describe("WEB-U-001 transient queue position", () => {
  it.each([0, 1, 2, 10000])("preserves the valid ahead count %i", (ahead) => {
    const value = {
      ...ready,
      sandbox: { ...ready.sandbox, state: "queued" },
      queue: { ahead },
    };
    expect(parseFrame(JSON.stringify(value))).toEqual(value);
  });

  it.each([-1, 1.5, 10001, "2", null])(
    "rejects the invalid ahead count %j at the wire boundary",
    (ahead) => {
      expect(() =>
        parseFrame(
          JSON.stringify({
            ...ready,
            sandbox: { ...ready.sandbox, state: "queued" },
            queue: { ahead },
          }),
        ),
      ).toThrow();
    },
  );

  // The queue says what the run waits for; a reason from a newer gateway is shown neutrally.
  it("says whether the run waits for a slot or for the workspace", () => {
    const queued = (queue: object) =>
      apply(
        emptyTranscript(),
        parseFrame(
          JSON.stringify({
            ...ready,
            sandbox: { ...ready.sandbox, state: "queued" },
            queue,
          }),
        ),
      );
    expect(runPresentation(queued({ ahead: 1, reason: "slot" }), 0).label).toBe(
      "Waiting for a free slot · 1 ahead",
    );
    expect(runPresentation(queued({ ahead: 1 }), 0).label).toBe(
      "Waiting for a free slot · 1 ahead",
    );
    expect(
      runPresentation(queued({ ahead: 0, reason: "workspace" }), 0).label,
    ).toBe("Waiting for the workspace");
    // A UX review before 17.0: why, in words, and no capital where no other desktop string has one. WEB-U-397:
    // a finished task's desktop gives way (GW-I-305), so it frees soon after one finishes.
    expect(
      runPresentation(queued({ ahead: 0, reason: "desktop_sessions" }), 0)
        .label,
    ).toBe(
      "Waiting for a free desktop: other tasks are using them all. One frees soon after another task finishes.",
    );
    // Desktop waiters have their own place in line (the gateway's desktop_positions): said as the slot's is.
    expect(
      runPresentation(queued({ ahead: 2, reason: "desktop_sessions" }), 0)
        .label,
    ).toBe("Waiting for a free desktop · 2 ahead");
    expect(
      runPresentation(queued({ ahead: 0, reason: "quota" }), 0).label,
    ).toBe("Queued");
    expect(() => queued({ ahead: 0, reason: 5 })).toThrow();
  });

  // WEB-U-121 (F26): the only wait is the person's own idle sandbox retiring, so nothing is queued from their side.
  it("WEB-U-121 reads Starting… while the person's own idle sandbox makes room", () => {
    const state = apply(
      emptyTranscript(),
      parseFrame(
        JSON.stringify({
          ...ready,
          sandbox: { ...ready.sandbox, state: "queued" },
          queue: { ahead: 0, reason: "own_sandbox" },
        }),
      ),
    );
    expect(runPresentation(state, 0).label).toBe("Starting…");
  });

  it("updates position without a sequence, a notice or changing recorded work", () => {
    let state = transcriptFrames.reduce(apply, emptyTranscript());
    const recorded = state;
    for (const [ahead, label] of [
      [2, "2 ahead"],
      [1, "1 ahead"],
      [0, "next"],
      [10000, "10\u202f000 ahead"],
    ] as const) {
      state = apply(state, {
        ...ready,
        sandbox: { ...ready.sandbox, state: "queued" },
        queue: { ahead },
      });
      expect(state).toMatchObject({ sandbox: "queued", queue: { ahead } });
      expect(state.lastSeq).toBe(recorded.lastSeq);
      expect(state.items).toBe(recorded.items);
      expect(state.runs).toBe(recorded.runs);
      expect(runPresentation(state, 0).label).toBe(
        `Waiting for a free slot · ${label}`,
      );
    }
    expect(recorded).toMatchObject({ sandbox: "idle", queue: null });
    state = apply(state, {
      ...ready,
      sandbox: { ...ready.sandbox, state: "creating" },
    });
    expect(state).toMatchObject({ sandbox: "creating", queue: null });
    expect(runPresentation(state, 0).label).toBe("Starting…");
  });

  it("does not invent a position for legacy frames or retain a prior position", () => {
    const waiting = apply(emptyTranscript(), {
      ...ready,
      sandbox: { ...ready.sandbox, state: "queued" },
      queue: { ahead: 2 },
    });
    for (const metadata of [{}, { queue: null }]) {
      const state = apply(waiting, {
        ...ready,
        ...metadata,
        sandbox: { ...ready.sandbox, state: "queued" },
      });
      expect(state).toMatchObject({ queue: null });
      expect(runPresentation(state, 0).label).toBe("Waiting for a free slot");
    }
    expect(apply(waiting, ready)).toMatchObject({ queue: null });
    expect(apply(waiting, { ...ready, queue: { ahead: 2 } })).toMatchObject({
      sandbox: "ready",
      queue: null,
    });
  });
});
