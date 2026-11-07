import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { frame, runId } from "../test/frames";

import { apply, emptyTranscript } from "./reducer";
import { StepStateProvider } from "./step-state";
import { TranscriptView } from "./TranscriptView";

/** A turn with two finished calls, then (when `done`) its run completed. */
function turn(done: boolean) {
  const call = (seq: number, id: string, name: string, args: object) => [
    frame(seq, { type: "tool.call", call_id: id, name, arguments: args }),
    frame(seq + 1, { type: "tool.start", call_id: id }),
    frame(seq + 2, { type: "tool.end", call_id: id, status: "ok" }),
    frame(seq + 3, {
      type: "tool.result",
      call_id: id,
      status: "ok",
      content: `${name} result`,
    }),
  ];
  const frames = [
    frame(1, { type: "message.user", text: "File the crash", attachments: [] }),
    frame(2, { type: "run.started", run_id: runId }),
    frame(3, { type: "step.started", step: 1, attempt: 1 }),
    ...call(4, "search", "shell", { command: "grep -r crash logs" }),
    ...call(8, "file", "shell", { command: "echo filed" }),
    ...(done
      ? [
          frame(12, { type: "text.delta", text: "Filed." }),
          frame(13, { type: "run.completed", run_id: runId }),
        ]
      : []),
  ];
  return frames
    .map((value, index) => ({ ...value, seq: index + 1 }))
    .reduce(apply, emptyTranscript());
}
const line = (name: RegExp) => screen.getByRole("button", { name });

// Seen in a real connector run, and reproduced on the fake front: a step the person opened while the turn ran
// disappeared when it finished, because the finished turn folded its steps over it. The person's choice outlives the
// turn's end, and any re-render or remount (a replay after a reconnect) of the transcript.
describe("WEB-U-016 a step the person opened stays open", () => {
  it("keeps an opened step open, and its group open, when the turn finishes", () => {
    const view = render(
      <StepStateProvider>
        <TranscriptView transcript={turn(false)} />
      </StepStateProvider>,
    );
    const first = /^Ran a command: grep -r crash logs/;
    expect(line(first)).toHaveAttribute("aria-expanded", "false");
    fireEvent.click(line(first));
    expect(line(first)).toHaveAttribute("aria-expanded", "true");
    view.rerender(
      <StepStateProvider>
        <TranscriptView transcript={turn(true)} />
      </StepStateProvider>,
    );
    expect(line(/^Worked for/)).toHaveAttribute("aria-expanded", "true");
    expect(line(first)).toHaveAttribute("aria-expanded", "true");
  });

  it("keeps it open when the transcript is rebuilt from a replay", () => {
    const steps = render(
      <StepStateProvider>
        <TranscriptView transcript={turn(true)} />
      </StepStateProvider>,
    );
    fireEvent.click(line(/^Worked for/));
    const first = /^Ran a command: grep -r crash logs/;
    fireEvent.click(line(first));
    // A reconnect replays the stream from nothing: the steps unmount, then come back.
    steps.rerender(
      <StepStateProvider>
        <TranscriptView transcript={emptyTranscript()} />
      </StepStateProvider>,
    );
    steps.rerender(
      <StepStateProvider>
        <TranscriptView transcript={turn(true)} />
      </StepStateProvider>,
    );
    expect(line(/^Worked for/)).toHaveAttribute("aria-expanded", "true");
    expect(line(first)).toHaveAttribute("aria-expanded", "true");
  });

  it("still folds a finished turn nobody opened", () => {
    render(
      <StepStateProvider>
        <TranscriptView transcript={turn(true)} />
      </StepStateProvider>,
    );
    expect(line(/^Worked for/)).toHaveAttribute("aria-expanded", "false");
  });
});
