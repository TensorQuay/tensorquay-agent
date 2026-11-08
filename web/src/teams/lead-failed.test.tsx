import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { english } from "../i18n/translate";
import { frame, runId } from "../test/frames";
import { carriesOnNext, composerReason } from "../transcript/presentation";
import { apply, emptyTranscript } from "../transcript/reducer";
import { RunStatus } from "../transcript/RunStatus";
import { TranscriptView } from "../transcript/TranscriptView";

import { leadCarriesOn } from "./lead-waiting";

const SHORT = "Waiting…";
const WHOLE =
  "Waiting for its helpers: its last step kept failing, and it carries on when a helper reports.";
const NEXT =
  "Its helpers are still working; it carries on when one reports. You can also send it a message.";
const NEXT_LOCKED =
  "Its helpers are still working; it carries on when one reports.";

const failed = [
  frame(1, { type: "message.user", text: "Plan the trip", attachments: [] }),
  frame(2, { type: "run.started", run_id: runId }),
  frame(3, {
    type: "run.failed",
    run_id: runId,
    code: "repeated_failure",
    reason: "the same step failed 3 times",
  }),
].reduce(apply, emptyTranscript());

// WEB-U-401 (a review before 17.0, the lead's approved design; real-world use before 17.0, 17 Oct 2026): the lead's
// header turned red "Failed" while both helpers worked; a helper's report woke it 15 s later and it finished. While its
// team runs and a helper works, a failed lead waits for its helpers: the header says so in the attention tone, the
// failure stays in the transcript with what happens next. Red Failed when nothing will wake it.
describe("WEB-U-401 a lead's failed run while its helpers work", () => {
  it("is waiting only while its team runs and a helper works, whatever its run (WEB-U-415)", () => {
    const carries = (over: Partial<Parameters<typeof leadCarriesOn>[0]>) =>
      leadCarriesOn(
        {
          lead: true,
          state: "running",
          helpersWorking: true,
          ...over,
        },
        english,
      );
    expect(carries({})).toEqual({
      short: SHORT,
      whole: WHOLE,
      next: NEXT,
      nextLocked: NEXT_LOCKED,
    });
    expect(carries({ helpersWorking: false })).toBeNull();
    expect(carries({ state: "ended" })).toBeNull();
    expect(carries({ state: "stopping" })).toBeNull();
    expect(carries({ lead: false })).toBeNull();
  });

  it("reads 'Waiting for its helpers' in the attention tone, the whole in its tooltip and read", () => {
    render(
      <RunStatus
        transcript={failed}
        leadCarriesOn={{ short: SHORT, whole: WHOLE }}
      />,
    );
    const group = screen.getByRole("group", { name: "Run status" });
    expect(group).toHaveAttribute("data-tone", "warn");
    expect(group).toHaveAttribute("data-short", SHORT);
    expect(group).toHaveAttribute("title", WHOLE);
    expect(screen.getByRole("status")).toHaveTextContent(WHOLE);
  });

  it("reads Failed in red without it", () => {
    render(<RunStatus transcript={failed} />);
    expect(screen.getByRole("group", { name: "Run status" })).toHaveAttribute(
      "data-tone",
      "danger",
    );
  });

  it("keeps the failure in the transcript, with what happens next", () => {
    render(<TranscriptView transcript={failed} failedNext={NEXT} />);
    const outcome = screen.getByText(NEXT).closest(".turn-outcome");
    expect(outcome).not.toBeNull();
    expect(outcome).toHaveTextContent(
      "The agent tried the same step several times and it kept failing",
    );
    expect(outcome).not.toHaveTextContent("Send a message to try another way.");
  });
});

// WEB-U-407 (17.0's release candidate; a live check before 17.0): under the failed step the page said
// "You can also send it a message" while the composer below said "You can reply once it starts." and took nothing: the
// lead's files were a helper's turn. The next step offers a message only when the composer takes one.
describe("WEB-U-407 a failed lead's next step offers a message only when the composer takes one", () => {
  const carries = leadCarriesOn(
    { lead: true, state: "running", helpersWorking: true },
    english,
  );
  it("offers one while the composer is open", () => {
    const open = composerReason(
      { id: runId, status: "failed" },
      "ready",
      false,
      english,
    );
    expect(open).toBeNull();
    expect(carriesOnNext(carries, open !== null)).toBe(NEXT);
  });

  it("leaves it out while the lead waits for its files, as the composer says", () => {
    const locked = composerReason(
      { id: runId, status: "failed" },
      "queued",
      false,
      english,
      { reason: "workspace", ahead: 0 },
    );
    expect(locked).not.toBeNull();
    expect(carriesOnNext(carries, locked !== null)).toBe(NEXT_LOCKED);
    expect(carriesOnNext(null, true)).toBeNull();
  });
});

// WEB-U-408 (17.0's release candidate): at 1280 the header's short word was cut, "Waiting for its …". WEB-U-155
// promises one short word at every width: "Waiting…", the whole in its tooltip, read, and under the failed step.
describe("WEB-U-408 the failed lead's header short word is one word", () => {
  it("reads Waiting… with the whole in its tooltip and read", () => {
    const carries = leadCarriesOn(
      { lead: true, state: "running", helpersWorking: true },
      english,
    );
    render(<RunStatus transcript={failed} leadCarriesOn={carries} />);
    const group = screen.getByRole("group", { name: "Run status" });
    expect(group).toHaveAttribute("data-short", "Waiting…");
    expect(group).toHaveAttribute("title", WHOLE);
    expect(screen.getByRole("status")).toHaveTextContent(WHOLE);
  });
});
