import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { frame, runId } from "../test/frames";

import { apply, emptyTranscript } from "./reducer";
import { RunStatus } from "./RunStatus";

const started = apply(
  emptyTranscript(),
  frame(1, { type: "run.started", run_id: runId }),
);
/** The run mark: before the status's words, next to the task's title in the bar. */
const mark = () => document.querySelector(".conversation-status > .run-mark");
const status = () => screen.getByLabelText("Run status");

// One run mark (17.3): a calm pulse next to the task's title while its run is under way, the same in the sidebar item
// and the tab title; "Starting…" and "Waiting for a free slot · 2 ahead" keep their words with it.
describe("WEB-U-441 one run mark", () => {
  it("pulses beside the status while the run works, hidden from assistive technology", () => {
    render(<RunStatus transcript={started} />);
    expect(status()).toHaveTextContent(/^Working…/);
    expect(mark()).toHaveAttribute("aria-hidden", "true");
  });

  it("keeps Starting… with the same mark", () => {
    render(
      <RunStatus transcript={{ ...emptyTranscript(), sandbox: "creating" }} />,
    );
    expect(status()).toHaveTextContent(/^Starting…$/);
    expect(mark()).not.toBeNull();
  });

  it("keeps Waiting for a free slot · 2 ahead with the same mark", () => {
    render(
      <RunStatus
        transcript={{
          ...emptyTranscript(),
          sandbox: "queued",
          queue: { ahead: 2 },
        }}
      />,
    );
    expect(status()).toHaveTextContent(/^Waiting for a free slot · 2 ahead$/);
    expect(mark()).not.toBeNull();
  });

  it("shows none once the run is done, nor for a message not sent", () => {
    const { unmount } = render(
      <RunStatus
        transcript={apply(
          started,
          frame(2, { type: "run.completed", run_id: runId }),
        )}
      />,
    );
    expect(status()).toHaveTextContent(/^Done/);
    expect(mark()).toBeNull();
    unmount();
    render(<RunStatus transcript={started} failure="runtime_start_failed" />);
    expect(mark()).toBeNull();
  });
});
