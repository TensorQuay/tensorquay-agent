import { act, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { frame, ready, runId } from "../test/frames";

import { apply, applyAt, emptyTranscript } from "./reducer";
import { RunStatus } from "./RunStatus";
import { TranscriptView } from "./TranscriptView";

describe("WEB-U-001 starting and failed work", () => {
  it("prioritizes the unacknowledged prompt over a busy environment or an early run frame", () => {
    const transcript = apply(
      emptyTranscript(),
      frame(1, { type: "run.started", run_id: runId }),
    );
    const { rerender } = render(
      <RunStatus transcript={{ ...transcript, sandbox: "busy" }} sending />,
    );
    expect(screen.getByLabelText("Run status")).toHaveTextContent(/^Sending…$/);
    rerender(<RunStatus transcript={transcript} />);
    expect(screen.getByLabelText("Run status")).toHaveTextContent(/^Working…/);
  });

  it("separates failed-work guidance from the recorded numeric summary", () => {
    const transcript = [
      frame(1, {
        type: "message.user",
        text: "Try a command",
        attachments: [],
      }),
      frame(2, { type: "run.started", run_id: runId }),
      frame(3, {
        type: "run.failed",
        run_id: runId,
        code: "io",
        reason: "Command could not complete",
      }),
    ].reduce(apply, emptyTranscript());
    render(<TranscriptView transcript={transcript} />);
    // The recorded reason is a diagnostic; the code's plain words and next step show (the owner's Mac test).
    expect(screen.queryByText(/Command could not complete/)).toBeNull();
    const reason = screen.getByText(
      "The task's environment couldn't be reached.",
    );
    expect(reason.closest(".turn-outcome")).toHaveTextContent(
      "Send the message again in a moment.",
    );
    expect(reason.closest(".type-label")).toBeNull();
    // A failed turn that changed nothing shows no files line at all (WEB-U-373); any summary line never repeats the
    // guidance.
    expect(
      reason.closest("article")?.querySelector(".turn-summary")?.textContent ??
        "",
    ).not.toMatch(/environment couldn't be reached|Send the message again/);
  });

  // Task 072: a failure code with its own copy is said once, as the turn's outcome, in plain words.
  it("says a fallback_parse failure once in plain words", () => {
    const transcript = [
      frame(1, { type: "message.user", text: "Build it", attachments: [] }),
      frame(2, { type: "run.started", run_id: runId }),
      frame(3, {
        type: "run.failed",
        run_id: runId,
        code: "fallback_parse",
        reason: "fallback parse failed after retry",
      }),
    ].reduce(apply, emptyTranscript());
    const { container } = render(<TranscriptView transcript={transcript} />);
    expect(
      screen.getByText("The model's answer couldn't be used."),
    ).toBeVisible();
    expect(container).not.toHaveTextContent(/fallback parse failed/);
    expect(
      container.textContent?.match(/answer couldn't be used/g),
    ).toHaveLength(1);
  });
});

// A pending replacement counts down while the page is idle too (WEB-U-068).
describe("WEB-U-068 the recovery countdown", () => {
  it("counts from when the notice arrives, not from when the page was idle since", () => {
    vi.useFakeTimers({ now: Date.parse("2026-09-28T08:00:00Z") });
    try {
      const idle = emptyTranscript();
      const { container, rerender } = render(<RunStatus transcript={idle} />);
      // Ten idle seconds pass with no clock running, then a loss is reported.
      act(() => {
        vi.setSystemTime(Date.parse("2026-09-28T08:00:10Z"));
      });
      const lost = applyAt(
        idle,
        {
          ...ready,
          sandbox: {
            ...ready.sandbox,
            state: "destroyed",
            recovery: { losses: 1, retry_in_secs: 3 },
          },
        },
        Date.now(),
      );
      rerender(<RunStatus transcript={lost} />);
      // The visible line, not the announcement beside it (which always gives the notice's own seconds).
      expect(container.querySelector(".environment-status")).toHaveTextContent(
        /^Environment stopped once; restarting in 3 s$/,
      );
    } finally {
      vi.useRealTimers();
    }
  });

  it("ticks down to the restart without a working run", () => {
    vi.useFakeTimers({ now: Date.parse("2026-09-28T08:00:00Z") });
    try {
      const transcript = applyAt(
        emptyTranscript(),
        {
          ...ready,
          sandbox: {
            ...ready.sandbox,
            state: "destroyed",
            recovery: { losses: 2, retry_in_secs: 5 },
          },
        },
        Date.now(),
      );
      render(<RunStatus transcript={transcript} />);
      const line = screen.getByLabelText("Run status").parentElement;
      expect(line).toHaveTextContent("restarting in 5 s");
      // Announced once as the notice gave it; the ticking countdown stays out of the live region.
      const announced = screen.getByRole("status");
      expect(announced).toHaveTextContent(
        "Environment stopped 2 times; restarting in 5 s",
      );
      act(() => {
        vi.advanceTimersByTime(3_000);
      });
      expect(line).toHaveTextContent("restarting in 2 s");
      expect(announced).toHaveTextContent("restarting in 5 s");
      act(() => {
        vi.advanceTimersByTime(3_000);
      });
      expect(line).toHaveTextContent("restarting now");
    } finally {
      vi.useRealTimers();
    }
  });
});

// Found before 17.0: the header's status gives way before its controls. The stylesheet truncates it (whole in its
// tooltip) and, under 768 px, shows a failure's short word; its reason is also the turn's outcome in the transcript.
describe("WEB-U-112 a status that gives way", () => {
  const failed = [
    frame(1, { type: "message.user", text: "Try it", attachments: [] }),
    frame(2, { type: "run.started", run_id: runId }),
    frame(3, {
      type: "run.failed",
      run_id: runId,
      code: "repeated_failure",
      reason: "same call three times",
    }),
  ].reduce(apply, emptyTranscript());

  it("has a failed run's short word, and keeps its whole status for reading and the tooltip", () => {
    render(<RunStatus transcript={failed} />);
    const group = screen.getByRole("group", { name: "Run status" });
    const status = screen.getByRole("status");
    expect(status.textContent).toMatch(/^Failed: .{12,}/);
    expect(group).toHaveAttribute("title", status.textContent);
    // The short word is the stylesheet's, so the status's text stays the whole.
    expect(group).toHaveAttribute("data-short", "Failed");
    expect(group).toHaveTextContent(/^Failed: .{12,}$/);
    expect(status).toHaveClass("run-long");
  });

  it("has a message not sent's short word", () => {
    render(
      <RunStatus transcript={emptyTranscript()} failure="Try again later." />,
    );
    const group = screen.getByRole("group", { name: "Run status" });
    expect(group).toHaveAttribute("data-short", "Not sent");
    expect(screen.getByRole("status")).toHaveTextContent(/Try again later/);
  });

  it("has no short word when the status is already short", () => {
    render(<RunStatus transcript={emptyTranscript()} />);
    expect(
      screen.getByRole("group", { name: "Run status" }),
    ).not.toHaveAttribute("data-short");
  });
});

// The 13f screen review, #4: at 1024 the header cut both the title and a long status ("Failed: The task's environment
// couldn't…", "Working… · …"). The status is one short word at every width, whole in its tooltip and for assistive
// technology; how long it has worked is in the tooltip and the transcript's live line (WEB-U-123).
describe("WEB-U-155 the header's status is one short word", () => {
  const working = [
    frame(1, { type: "message.user", text: "Try it", attachments: [] }),
    frame(2, { type: "run.started", run_id: runId }),
    frame(3, { type: "step.started", step: 1, attempt: 1 }),
  ].reduce(apply, emptyTranscript());

  it("words a working run short, with how long in its tooltip", () => {
    render(<RunStatus transcript={working} />);
    const group = screen.getByRole("group", { name: "Run status" });
    expect(group).toHaveAttribute("data-short", "Working…");
    expect(group.getAttribute("title")).toMatch(/^Working… · .+/);
    expect(screen.getByRole("status")).toHaveTextContent(/^Working…$/);
  });

  // A pass over the pages before 17.0: at 390 the desktop wait's sentence was cut mid-word ("…using them a…"), when it
  // frees lost; a longer short squeezed the title to a few letters. The header says "Waiting…"; the composer says when
  // it frees. WEB-U-397 (real-world use before 17.0): a finished task's desktop now gives way to a waiting one
  // (GW-I-305), so the line no longer promises an idle wait.
  it("words a wait for a free desktop short, the whole in its tooltip and read", () => {
    const whole =
      "Waiting for a free desktop: other tasks are using them all. One frees soon after another task finishes.";
    const waiting = apply(emptyTranscript(), {
      ...ready,
      sandbox: { backend: "docker", state: "queued" },
      queue: { ahead: 0, reason: "desktop_sessions" },
    });
    render(<RunStatus transcript={waiting} />);
    const group = screen.getByRole("group", { name: "Run status" });
    expect(group).toHaveAttribute("data-short", "Waiting…");
    expect(group).toHaveAttribute("title", whole);
    expect(screen.getByRole("status")).toHaveTextContent(whole);
  });
});

// Page pass 14b, T rows: an opened task whose last run completed read "Ready" for about a quarter of a second, then
// "Done". Until the stream replays a frame, the header shows the task's stored run status; the replay then takes over.
describe("WEB-U-249 an opened task's status before its replay", () => {
  const status = () => screen.getByRole("status");

  it("shows how the last run ended, never Ready, until a frame arrives", () => {
    const { rerender } = render(
      <RunStatus transcript={emptyTranscript()} stored="completed" />,
    );
    expect(status()).toHaveTextContent(/^Done$/);
    rerender(<RunStatus transcript={emptyTranscript()} stored="cancelled" />);
    expect(status()).toHaveTextContent(/^Stopped$/);
    rerender(<RunStatus transcript={emptyTranscript()} stored="failed" />);
    // The reason comes with the replay; until then the state alone.
    expect(status()).toHaveTextContent(/^Failed$/);
    rerender(<RunStatus transcript={emptyTranscript()} stored="running" />);
    expect(status()).toHaveTextContent(/^Working…$/);
    rerender(<RunStatus transcript={emptyTranscript()} stored="idle" />);
    expect(status()).toHaveTextContent(/^Ready$/);
  });

  it("gives way to the replayed transcript", () => {
    const replayed = [
      frame(1, { type: "message.user", text: "Try it", attachments: [] }),
      frame(2, { type: "run.started", run_id: runId }),
    ].reduce(apply, emptyTranscript());
    render(<RunStatus transcript={replayed} stored="completed" />);
    expect(status()).toHaveTextContent(/^Working…/);
  });

  // c15's WEB-E2E-018: a fresh task whose first message waits has no frames until it's admitted, so a reload read
  // its stored "idle" as "Ready" while the snapshot said queued.
  it("gives way to a queued snapshot, though no frame has arrived", () => {
    const waiting = apply(emptyTranscript(), {
      ...ready,
      sandbox: { backend: "docker", state: "queued" },
      queue: { ahead: 0, reason: "slot" },
    });
    expect(waiting.lastSeq).toBe(0);
    const { rerender } = render(
      <RunStatus transcript={waiting} stored="idle" />,
    );
    expect(status()).toHaveTextContent(/^Waiting for a free slot · next$/);
    rerender(
      <RunStatus
        transcript={{ ...waiting, queue: { ahead: 2, reason: "slot" } }}
        stored="idle"
      />,
    );
    expect(status()).toHaveTextContent(/^Waiting for a free slot · 2 ahead$/);
  });

  it("gives way to a starting environment, though no frame has arrived", () => {
    const starting = apply(emptyTranscript(), {
      ...ready,
      sandbox: { backend: "docker", state: "creating" },
    });
    expect(starting.lastSeq).toBe(0);
    render(<RunStatus transcript={starting} stored="idle" />);
    expect(status()).toHaveTextContent(/^Starting…$/);
  });

  it("shows a fresh task's lost environment, though no frame has arrived", () => {
    // Stored "idle" (no run yet) reads as the live state does, with the recovery line (WEB-U-068).
    vi.useFakeTimers({ now: Date.parse("2026-09-28T08:00:00Z") });
    try {
      const lost = applyAt(
        emptyTranscript(),
        {
          ...ready,
          sandbox: {
            ...ready.sandbox,
            state: "destroyed",
            recovery: { losses: 1, retry_in_secs: 3 },
          },
        },
        Date.now(),
      );
      expect(lost.lastSeq).toBe(0);
      const { container } = render(
        <RunStatus transcript={lost} stored="idle" />,
      );
      expect(container.querySelector(".environment-status")).toHaveTextContent(
        /^Environment stopped once; restarting in 3 s$/,
      );
    } finally {
      vi.useRealTimers();
    }
  });

  it("keeps the stored status over an idle environment's snapshot until a frame arrives", () => {
    // The stream opens with its snapshot, then replays: a finished task must not read "Ready" in between.
    const opened = apply(emptyTranscript(), ready);
    render(<RunStatus transcript={opened} stored="completed" />);
    expect(status()).toHaveTextContent(/^Done$/);
  });
});
