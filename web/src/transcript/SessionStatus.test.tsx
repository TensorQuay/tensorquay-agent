import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { frame, ready, runId } from "../test/frames";

import { Composer } from "./Composer";
import { apply, emptyTranscript } from "./reducer";
import { runPresentation } from "./run-status";
import { RunStatus } from "./RunStatus";

describe("WEB-U-001 queued, sleeping and waking copy", () => {
  it("announces queue changes including the next position", () => {
    const state = emptyTranscript();
    const { rerender } = render(<RunStatus transcript={state} />);
    for (const ahead of [2, 1, 0]) {
      const queued = apply(state, {
        ...ready,
        sandbox: { ...ready.sandbox, state: "queued" },
        queue: { ahead },
      });
      rerender(<RunStatus transcript={queued} />);
      expect(screen.getByRole("status")).toHaveTextContent(
        `Waiting for a free slot · ${ahead === 0 ? "next" : `${ahead} ahead`}`,
      );
    }
  });

  it.each(["idle", "destroyed"] as const)(
    "says nothing of a sleeping environment (%s) and permits the next message to wake it",
    (sandbox) => {
      const completed = [
        frame(1, { type: "run.started", run_id: runId }),
        frame(2, { type: "run.completed", run_id: runId }),
      ].reduce(apply, emptyTranscript());
      // Finding T-d: people don't manage environments; a sleeping one isn't news in the header.
      expect(
        runPresentation({ ...completed, sandbox }, 0).environment,
      ).toBeNull();
      const send = vi.fn(() => Promise.resolve(true));
      render(
        <Composer
          run={completed.run}
          sandbox={sandbox}
          sending={false}
          cancelling={false}
          onSend={send}
          onStop={vi.fn()}
        />,
      );
      const input = screen.getByRole("textbox", { name: "Message" });
      expect(input).toBeEnabled();
      fireEvent.change(input, { target: { value: "Wake and continue" } });
      fireEvent.keyDown(input, { key: "Enter" });
      expect(send).toHaveBeenCalledExactlyOnceWith("Wake and continue");
    },
  );

  it("keeps a new conversation calm and distinguishes waking from paused approval", () => {
    expect(runPresentation(emptyTranscript(), 0)).toMatchObject({
      label: "Ready",
      environment: null,
    });
    expect(
      runPresentation({ ...emptyTranscript(), sandbox: "creating" }, 0),
    ).toMatchObject({ label: "Starting…", environment: null });
  });

  it("says in the composer what to do while queued, and prevents a second send", () => {
    const send = vi.fn(() => Promise.resolve(true));
    render(
      <Composer
        run={emptyTranscript().run}
        sandbox="queued"
        sending={false}
        cancelling={false}
        onSend={send}
        onStop={vi.fn()}
      />,
    );
    const input = screen.getByRole("textbox", { name: "Message" });
    // review-050: one queued message - the helper line - and the ordinary placeholder.
    expect(input).toHaveAttribute("placeholder", "Reply…");
    // WEB-U-161: the composer says what to do; the state itself is the header's.
    expect(screen.getByText("You can reply once it starts.")).toBeVisible();
    expect(input).toBeDisabled();
    expect(screen.getByRole("button", { name: "Send" })).toBeDisabled();
    fireEvent.keyDown(input, { key: "Enter" });
    expect(send).not.toHaveBeenCalled();
  });
});
